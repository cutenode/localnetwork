import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { lock } from '../src/lock.ts';

const path = () => join(mkdtempSync(join(tmpdir(), 'atstore-lock-')), 'work.lock');

describe('lock', () => {
	it('holds the lock with this process, and lets it go', () => {
		const at = path();
		const unlock = lock(at);

		expect(readFileSync(at, 'utf8')).toBe(String(process.pid));
		unlock();
		expect(existsSync(at)).toBe(false);
	});

	it('refuses a lock a running process holds, naming it', () => {
		const at = path();
		const unlock = lock(at);

		expect(() => lock(at)).toThrow(`${at} is held by another run, process ${process.pid}`);
		unlock();
		expect(() => lock(at)()).not.toThrow();
	});

	it("refuses a lock held by another user's process, which it can't signal", () => {
		const at = path();
		// launchd on macOS, init or systemd on Linux: always running, and never this user's.
		writeFileSync(at, '1');

		expect(() => lock(at)).toThrow('process 1');
	});

	it('takes over a lock whose process has gone, or that holds no process', () => {
		for (const held of [String(spawnSync('true').pid), '']) {
			const at = path();
			writeFileSync(at, held);

			const unlock = lock(at);
			expect(readFileSync(at, 'utf8')).toBe(String(process.pid));
			unlock();
		}
	});

	it("fails as the file system does when the lock can't be made", () => {
		expect(() => lock(join(tmpdir(), 'atstore-no-such-folder', 'work.lock'))).toThrow(/ENOENT/);
	});
});
