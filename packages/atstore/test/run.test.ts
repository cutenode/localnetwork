import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { run, start } from '../src/run.ts';

/** Whether a process with this pid is still running. */
const alive = (pid: number) => {
	try {
		process.kill(pid, 0);

		return true;
	} catch {
		return false;
	}
};

describe('run', () => {
	it('runs a command to its end, with the environment it is given', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'atstore-run-'));
		await run('sh', ['-c', 'echo "$WORD" > said'], { cwd: dir, env: { WORD: 'hello' } });
		expect(readFileSync(join(dir, 'said'), 'utf8')).toBe('hello\n');
	});

	it('fails when the command does', async () => {
		await expect(run('sh', ['-c', 'exit 3'])).rejects.toThrow('sh -c exit 3 exited with 3');
	});
});

describe('start', () => {
	it('is ready once a line matches, and stops what the command started', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'atstore-start-'));

		// A server stand-in that starts a child of its own, as dev-network.ts starts the app.
		const running = await start(
			'sh',
			['-c', 'sleep 60 & echo $! > child; echo booting; echo >&2 "Listening on 1"; wait'],
			{
				cwd: dir,
				ready: /^Listening on/
			}
		);

		const child = Number(readFileSync(join(dir, 'child'), 'utf8'));
		expect(alive(child)).toBe(true);

		await running.stop();
		await expect.poll(() => alive(child)).toBe(false);
		await running.stop();
	});

	it('fails with the output when the command ends before it is ready', async () => {
		await expect(start('sh', ['-c', 'echo "no port"; exit 1'], { ready: /Listening/ })).rejects.toThrow(
			/exited with 1 before it was ready:\nno port/
		);
	});

	it('gives up, and stops it, when nothing matches in time', async () => {
		await expect(start('sh', ['-c', 'echo waiting; sleep 60'], { ready: /Listening/, timeout: 300 })).rejects.toThrow(
			/wasn't ready in 0.3s:\nwaiting/
		);
	});
});
