import { execFile } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { takeBackup } from '../src/lib/server/backup/backups';
import { type BackupStore, createStore } from '../src/lib/server/backup/store';
import { NodeSqliteDatabase } from '../src/lib/server/db/sqlite';
import { silentLogger } from '../src/lib/server/logger';
import { fakeS3 } from '../src/lib/server/testing/s3';

const run = promisify(execFile);

const script = join(import.meta.dirname, 'db-backup.ts');

let dir: string;

let s3: Awaited<ReturnType<typeof fakeS3>>;

let store: BackupStore;

// The longest a run of the script may take before it is killed. One takes 0.3 to 1.3s, and on a busy
// machine (these tests at background priority next to other tests or busy loops) up to about 15s.
const SCRIPT_TIMEOUT = 30_000;

// Run with plain Node, as in the production image, so this also checks that Node can strip the
// types of everything it imports. A run still going after `timeout` is killed, and the error says
// what it was waiting on.
const dbBackup = (args: string[], env: Record<string, string> = {}, { bucket = s3, timeout = SCRIPT_TIMEOUT } = {}) =>
	run(process.execPath, [script, ...args], {
		cwd: dir,
		env: { PATH: process.env.PATH, NODE_ENV: 'production', DATABASE_PATH: join(dir, 'app.db'), ...env },
		timeout,
		killSignal: 'SIGKILL'
	}).then(
		(result) => ({ code: 0, ...result }),
		(error: { code: number; killed?: boolean; stdout: string; stderr: string }) => {
			if (!error.killed) return error;

			const waitingOn = bucket.unanswered();

			throw new Error(
				`db-backup.ts ${args.join(' ')} was still running after ${timeout / 1000}s, ${
					waitingOn.length ? `waiting on the bucket for ${waitingOn.join(', ')}` : 'with no request to the bucket open'
				}. It printed: ${JSON.stringify(error.stdout + error.stderr)}`
			);
		}
	);

beforeEach(async () => {
	dir = await mkdtemp(join(tmpdir(), 'db-backup-script-'));
	s3 = await fakeS3();
	store = createStore(s3.settings);
	const db = new NodeSqliteDatabase(join(dir, 'app.db'));
	db.raw.exec("CREATE TABLE people (name TEXT); INSERT INTO people VALUES ('ana')");
	db.close();
});

afterEach(async () => {
	store.close();
	await s3.close();
	await rm(dir, { recursive: true, force: true });
});

// One run of the script per test: each one starts Node and loads the S3 client. The deadline sits
// above the script's own, so a hung run fails with the error above rather than a generic timeout.
describe('scripts/db-backup.ts', { timeout: SCRIPT_TIMEOUT + 10_000 }, () => {
	const backUp = () =>
		takeBackup({ databasePath: join(dir, 'app.db'), store, keep: s3.settings.keep, logger: silentLogger });

	it('backs the database up', async () => {
		expect((await dbBackup(['backup'], s3.env)).code).toBe(0);
		expect([...s3.objects.keys()]).toEqual([expect.stringMatching(/^localnetwork-db\/.+\.sqlite\.gz$/)]);
	});

	it('lists the backups', async () => {
		const { key } = await backUp();

		expect((await dbBackup(['list'], s3.env)).stdout).toContain(key);
	});

	it('restores the latest backup to a path', async () => {
		await backUp();

		expect((await dbBackup(['restore', 'latest', join(dir, 'restored.db')], s3.env)).code).toBe(0);
		const restored = new NodeSqliteDatabase(join(dir, 'restored.db'));
		expect(restored.raw.prepare('SELECT name FROM people').all()).toEqual([{ name: 'ana' }]);
		restored.close();
	});

	it('fails when the bucket has no backups to restore', async () => {
		const result = await dbBackup(['restore', 'latest'], s3.env);

		expect(result.code).toBe(1);
		expect(result.stdout).toContain('the bucket has no backups yet');
	});

	it('says what to set when backups are off', async () => {
		const result = await dbBackup(['backup']);

		expect(result.code).toBe(1);
		expect(result.stderr).toContain('Set BACKUP_S3_BUCKET');
	});

	it('says how to call it', async () => {
		const result = await dbBackup(['restore'], s3.env);

		expect(result.code).toBe(2);
		expect(result.stderr).toContain('usage: db-backup.ts');
	});

	it('stops a run the bucket never answers, and says what it was waiting on', async () => {
		const silent = await fakeS3({ silent: true });

		try {
			const outcome = await Promise.allSettled([dbBackup(['list'], silent.env, { bucket: silent, timeout: 2000 })]);

			// On a busy machine the run can be killed before it gets as far as asking the bucket.
			const waitingOn = silent.unanswered().length
				? 'waiting on the bucket for GET /backups/?list-type=2&prefix=localnetwork-db%2F'
				: 'with no request to the bucket open';

			expect(outcome).toEqual([
				{
					status: 'rejected',
					reason: new Error(`db-backup.ts list was still running after 2s, ${waitingOn}. It printed: ""`)
				}
			]);
		} finally {
			await silent.close();
		}
	});
});
