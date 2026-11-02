import { closeSync, mkdtempSync, openSync, readFileSync, rmSync, statfsSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { mountSticks } from '../src/sticks.ts';

/** Whether something is mounted at `path`: a file system of its own, apart from its folder's. */
const mounted = (path: string) => {
	try {
		return statfsSync(path).type !== statfsSync(join(path, '..')).type;
	} catch {
		return false;
	}
};

// hdiutil and diskutil are macOS's; elsewhere `pnpm atstore` can't make locallibrary's captures either.
describe.runIf(process.platform === 'darwin')('mountSticks', () => {
	let dir = '';

	afterEach(() => {
		rmSync(dir, { recursive: true, force: true });
	});

	it('mounts a FAT32 stick for each name, its size, and ejects them all', async () => {
		dir = mkdtempSync('/tmp/atstore-sticks-');
		const sticks = await mountSticks(dir, ['ONE', 'TWO']);
		const [one, two] = [join(sticks.volumes, 'ONE'), join(sticks.volumes, 'TWO')];

		try {
			expect(sticks.volumes).toBe(join(dir, 'Volumes'));
			expect([mounted(one), mounted(two)]).toEqual([true, true]);
			const { bsize, blocks } = statfsSync(one);
			expect(bsize * blocks).toBeGreaterThan(31.9e9);
			expect(bsize * blocks).toBeLessThanOrEqual(32e9);
			writeFileSync(join(one, 'track.wav'), 'audio');
			expect(readFileSync(join(one, 'track.wav'), 'utf8')).toBe('audio');
		} finally {
			await sticks.eject();
		}

		expect([mounted(one), mounted(two)]).toEqual([false, false]);
		await expect(sticks.eject()).resolves.toBeUndefined();
	}, 60_000);

	it('ejects a stick that a process still has a file open on', async () => {
		dir = mkdtempSync('/tmp/atstore-sticks-');
		const sticks = await mountSticks(dir, ['BUSY']);
		const busy = join(sticks.volumes, 'BUSY');
		const file = openSync(join(busy, 'open.wav'), 'w');

		try {
			await sticks.eject();
		} finally {
			closeSync(file);
		}

		expect(mounted(busy)).toBe(false);
	}, 60_000);

	it('ejects the sticks it made when a later one fails', async () => {
		dir = mkdtempSync('/tmp/atstore-sticks-');

		// The image of a name with a slash in it would go in a folder that isn't there.
		await expect(mountSticks(dir, ['ONE', 'NO/PE'])).rejects.toThrow(/hdiutil create/);
		expect(mounted(join(dir, 'Volumes/ONE'))).toBe(false);
	}, 60_000);
});
