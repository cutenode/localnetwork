import {
	closeSync,
	mkdirSync,
	mkdtempSync,
	openSync,
	readFileSync,
	rmSync,
	statfsSync,
	symlinkSync,
	writeFileSync
} from 'node:fs';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { ejectLeftovers, mountSticks } from '../src/sticks.ts';

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
	const dirs: string[] = [];

	const tmp = () => {
		const dir = mkdtempSync('/tmp/atstore-sticks-');
		dirs.push(dir);

		return dir;
	};

	// Whatever a failed test left mounted goes before its folder does.
	afterEach(async () => {
		for (const dir of dirs.splice(0)) {
			await ejectLeftovers(dir);
			rmSync(dir, { recursive: true, force: true });
		}
	});

	it('mounts a FAT32 stick for each name, its size, and ejects them all', async () => {
		const dir = tmp();
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
			expect(await sticks.eject()).toBe(true);
		}

		expect([mounted(one), mounted(two)]).toEqual([false, false]);
		await expect(sticks.eject()).resolves.toBe(true);
	}, 60_000);

	it('ejects a stick that a process still has a file open on', async () => {
		const sticks = await mountSticks(tmp(), ['BUSY']);
		const busy = join(sticks.volumes, 'BUSY');
		const file = openSync(join(busy, 'open.wav'), 'w');

		try {
			expect(await sticks.eject()).toBe(true);
		} finally {
			closeSync(file);
		}

		expect(mounted(busy)).toBe(false);
	}, 60_000);

	it('ejects the sticks it made when a later one fails to be made', async () => {
		const dir = tmp();

		// The image of a name with a slash in it would go in a folder that isn't there.
		await expect(mountSticks(dir, ['ONE', 'NO/PE'])).rejects.toThrow(/hdiutil create/);
		expect(mounted(join(dir, 'Volumes/ONE'))).toBe(false);
	}, 60_000);

	it('ejects the sticks it made when a later one fails to mount', async () => {
		const dir = tmp();
		// TAKEN's mount point is ONE's, which ONE is mounted on by then.
		mkdirSync(join(dir, 'Volumes'));
		symlinkSync(join(dir, 'Volumes/ONE'), join(dir, 'Volumes/TAKEN'));

		await expect(mountSticks(dir, ['ONE', 'TAKEN'])).rejects.toThrow(/diskutil image attach/);
		expect(mounted(join(dir, 'Volumes/ONE'))).toBe(false);
	}, 60_000);

	it('says which stick would not eject, and that not all went', async () => {
		const sticks = await mountSticks(tmp(), ['GONE']);
		// Ejected behind its back, so its own eject finds nothing at the mount point.
		await expect(ejectLeftovers(join(sticks.volumes, '..'))).resolves.toBe(true);

		expect(await sticks.eject()).toBe(false);
	}, 60_000);

	describe('ejectLeftovers', () => {
		it('ejects whatever a run left mounted in its folder, and nothing else', async () => {
			const dir = tmp();
			await mountSticks(dir, ['LEFT', 'OVER']);
			mkdirSync(join(dir, 'Volumes/EMPTY'));

			expect(await ejectLeftovers(dir)).toBe(true);
			expect([mounted(join(dir, 'Volumes/LEFT')), mounted(join(dir, 'Volumes/OVER'))]).toEqual([false, false]);
		}, 60_000);

		it("leaves a stick of another folder's alone, even with a symlink to it in this one", async () => {
			const other = tmp();
			await mountSticks(other, ['THEIRS']);
			const dir = tmp();
			mkdirSync(join(dir, 'Volumes'));
			symlinkSync(join(other, 'Volumes/THEIRS'), join(dir, 'Volumes/THEIRS'));

			expect(await ejectLeftovers(dir)).toBe(true);
			expect(mounted(join(other, 'Volumes/THEIRS'))).toBe(true);
		}, 60_000);

		it('has nothing to do in a folder with no sticks', async () => {
			await expect(ejectLeftovers(tmp())).resolves.toBe(true);
		});
	});
});
