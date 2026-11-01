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

// Run with plain Node, as in the production image, so this also checks that Node can strip the
// types of everything it imports.
const dbBackup = (args: string[], env: Record<string, string> = {}) =>
	run(process.execPath, [script, ...args], {
		cwd: dir,
		env: { PATH: process.env.PATH, NODE_ENV: 'production', DATABASE_PATH: join(dir, 'app.db'), ...env }
	}).then(
		(result) => ({ code: 0, ...result }),
		(error: { code: number; stdout: string; stderr: string }) => error
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

// One run of the script per test: each one starts Node and loads the S3 client.
describe('scripts/db-backup.ts', () => {
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
});
