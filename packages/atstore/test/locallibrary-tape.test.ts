import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { killPlayers, tape, TERMINALS, VIEWS } from '../src/listings/locallibrary-tape.ts';

const env = { LOCALLIBRARY_AUDIO: 'silent' };

describe('tape', () => {
	for (const terminal of TERMINALS) {
		const lines = tape('/bin/locallibrary', env, '/tmp/work/Volumes', terminal).split('\n');
		const name = `${terminal.width}x${terminal.height} at ${terminal.fontSize}px`;

		it(`quits every player it opens before the next, and before it ends (${name})`, () => {
			const steps = lines.flatMap((line) => {
				if (line.includes('locallibrary tui ')) return ['open'];

				return line === 'Ctrl+C' ? ['quit'] : [];
			});

			expect(steps).toEqual(['open', 'quit', 'open', 'quit']);
		});

		it(`screenshots the terminal's views alone, at its size (${name})`, () => {
			expect(lines).toContain(`Set Width ${terminal.width}`);
			expect(lines).toContain(`Set FontSize ${terminal.fontSize}`);
			expect(lines.filter((line) => line.startsWith('Screenshot '))).toEqual(
				terminal.views.map((view) => `Screenshot "${view}${terminal.suffix}.png"`)
			);
			expect(lines).toContain('Env LOCALLIBRARY_AUDIO "silent"');
		});
	}

	it('screenshots every view wide and for phones, once each, at the ratios the website shows them in', () => {
		for (const suffix of ['', '-phone']) {
			const terminals = TERMINALS.filter((terminal) => terminal.suffix === suffix);
			expect(terminals.flatMap((terminal) => terminal.views).toSorted()).toEqual(VIEWS.toSorted());
		}

		const ratios = new Set(TERMINALS.map((terminal) => `${terminal.suffix} ${terminal.width / terminal.height}`));
		expect(ratios).toEqual(new Set([` ${2560 / 1600}`, `-phone ${780 / 1400}`]));
	});
});

describe('killPlayers', () => {
	it('kills the players still open on the drives in the folder, and nothing else', async () => {
		const work = mkdtempSync(join(tmpdir(), 'atstore-players-'));
		// Stand-ins, with the arguments a player opened by the tape has.
		const wait = ['-e', 'setTimeout(() => {}, 60_000)'];
		const player = spawn(process.execPath, [...wait, 'locallibrary', 'tui', join(work, 'Volumes/DJ')]);
		const other = spawn(process.execPath, [...wait, 'locallibrary', 'tui', '/tmp/elsewhere/Volumes/DJ']);
		await Promise.all([once(player, 'spawn'), once(other, 'spawn')]);

		try {
			expect(killPlayers(work)).toEqual([player.pid]);
			const [, signal] = await once(player, 'exit');
			expect(signal).toBe('SIGKILL');
			expect(other.exitCode).toBeNull();
			expect(killPlayers(work)).toEqual([]);
		} finally {
			other.kill();
		}
	});
});
