import { existsSync } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { NodeSqliteDatabase } from '../db/sqlite';
import { silentLogger } from '../logger';
import { fakeS3 } from '../testing/s3';
import { backupKey, dailyBackups, listBackups, restoreBackup, takeBackup } from './backups';
import { type BackupStore, createStore } from './store';

let dir: string;

let s3: Awaited<ReturnType<typeof fakeS3>>;

let store: BackupStore;

let live: NodeSqliteDatabase;

let databasePath: string;

const names = (db: NodeSqliteDatabase) =>
	db.raw
		.prepare('SELECT name FROM people ORDER BY name')
		.all()
		.map((row) => row.name);

const keep = { daily: 7, weekly: 8 };

beforeEach(async () => {
	dir = await mkdtemp(join(tmpdir(), 'backups-test-'));
	s3 = await fakeS3();
	store = createStore(s3.settings);
	databasePath = join(dir, 'data', 'localnetwork.db');
	// Opened the way the app opens it (WAL), and left open, as the app leaves it while it runs.
	live = new NodeSqliteDatabase(databasePath);
	live.raw.exec("CREATE TABLE people (name TEXT); INSERT INTO people VALUES ('ana'), ('bo')");
});

afterEach(async () => {
	live.close();
	store.close();
	await s3.close();
	await rm(dir, { recursive: true, force: true });
});

describe('takeBackup and restoreBackup', () => {
	it('backs up a live database and restores it under the open connection', async () => {
		const { key, bytes } = await takeBackup({ databasePath, store, keep, logger: silentLogger });

		expect(key).toMatch(/^localnetwork-db\/\d{8}T\d{9}Z\.sqlite\.gz$/);
		expect(s3.objects.get(key)?.length).toBe(bytes);

		live.raw.exec("DELETE FROM people WHERE name = 'ana'; INSERT INTO people VALUES ('cy')");
		await restoreBackup({ key, targetPath: databasePath, store, logger: silentLogger });

		expect(names(live)).toEqual(['ana', 'bo']);
		// What it replaced is kept beside it.
		const previous = new NodeSqliteDatabase(`${databasePath}.before-restore`);
		expect(names(previous)).toEqual(['bo', 'cy']);
		previous.close();
	});

	it('restores to a new path, creating its directory', async () => {
		const { key } = await takeBackup({ databasePath, store, keep, logger: silentLogger });
		const copyPath = join(dir, 'elsewhere', 'copy.db');

		await restoreBackup({ key, targetPath: copyPath, store, logger: silentLogger });

		const copy = new NodeSqliteDatabase(copyPath);
		expect(names(copy)).toEqual(['ana', 'bo']);
		copy.close();
		expect(existsSync(`${copyPath}.before-restore`)).toBe(false);
	});

	it('refuses to restore a damaged backup and leaves the database alone', async () => {
		const key = backupKey(new Date('2026-10-30T00:10:00Z'));
		s3.objects.set(key, gzipSync('not a database'));

		await expect(restoreBackup({ key, targetPath: databasePath, store, logger: silentLogger })).rejects.toThrow(
			/not a database/
		);
		expect(names(live)).toEqual(['ana', 'bo']);
	});

	it("refuses keys that aren't backups, and a database that doesn't exist", async () => {
		await expect(
			restoreBackup({ key: 'other/file.txt', targetPath: databasePath, store, logger: silentLogger })
		).rejects.toThrow(/isn't a backup's key/);
		await expect(
			takeBackup({ databasePath: join(dir, 'missing.db'), store, keep, logger: silentLogger })
		).rejects.toThrow(/doesn't exist/);
		expect(existsSync(join(dir, 'missing.db'))).toBe(false);
	});

	it('deletes the backups the retention policy no longer keeps, and nothing else', async () => {
		s3.objects.set('notes.txt', Buffer.from('mine'));

		for (const day of ['2026-10-27', '2026-10-28', '2026-10-29']) {
			await takeBackup({
				databasePath,
				store,
				keep: { daily: 2, weekly: 0 },
				logger: silentLogger,
				now: new Date(`${day}T00:10:00Z`)
			});
		}

		expect([...s3.objects.keys()].sort()).toEqual([
			'localnetwork-db/20261028T001000000Z.sqlite.gz',
			'localnetwork-db/20261029T001000000Z.sqlite.gz',
			'notes.txt'
		]);
	});
});

describe('listBackups', () => {
	it('lists every page of the bucket, newest first', async () => {
		const paged = await fakeS3({ pageSize: 2 });
		const pagedStore = createStore(paged.settings);

		for (const day of ['2026-10-26', '2026-10-30', '2026-10-28', '2026-10-27', '2026-10-29']) {
			paged.objects.set(backupKey(new Date(`${day}T00:10:00Z`)), Buffer.from(''));
		}

		const listed = await listBackups(pagedStore);
		pagedStore.close();
		await paged.close();

		expect(listed.map((backup) => backup.takenAt.toISOString().slice(0, 10))).toEqual([
			'2026-10-30',
			'2026-10-29',
			'2026-10-28',
			'2026-10-27',
			'2026-10-26'
		]);
	});
});

describe('dailyBackups', () => {
	it('backs up on the first run of each UTC day only', async () => {
		s3.objects.set(backupKey(new Date('2026-10-29T00:10:00Z')), Buffer.from(''));
		const backups = dailyBackups({ databasePath, settings: s3.settings, logger: silentLogger });

		expect(await backups.runIfDue(new Date('2026-10-29T05:00:00Z'))).toBeUndefined();
		const taken = await backups.runIfDue(new Date('2026-10-30T00:20:00Z'));
		expect(taken?.key).toBe('localnetwork-db/20261030T002000000Z.sqlite.gz');
		expect(await backups.runIfDue(new Date('2026-10-30T01:20:00Z'))).toBeUndefined();
		expect(backups.status(new Date('2026-10-30T01:20:00Z'))).toEqual({
			latestAt: '2026-10-30T00:20:00.000Z',
			lastAttempt: { at: '2026-10-30T00:20:00.000Z', error: null },
			stale: false
		});
		backups.close();
	});

	it('keeps a failure for its status instead of throwing, and goes stale two days after the last backup', async () => {
		s3.objects.set(backupKey(new Date('2026-10-28T00:10:00Z')), Buffer.from(''));

		const backups = dailyBackups({
			databasePath: join(dir, 'missing.db'),
			settings: s3.settings,
			logger: silentLogger
		});

		expect(await backups.runIfDue(new Date('2026-10-29T00:20:00Z'))).toBeUndefined();
		expect(backups.status(new Date('2026-10-30T00:10:00Z'))).toEqual({
			latestAt: '2026-10-28T00:10:00.000Z',
			lastAttempt: { at: '2026-10-29T00:20:00.000Z', error: expect.stringContaining("doesn't exist") },
			stale: false
		});
		expect(backups.status(new Date('2026-10-30T00:11:00Z')).stale).toBe(true);
		backups.close();
	});

	it('counts the two days from when the app started while it knows of no backup', async () => {
		await s3.close();

		const backups = dailyBackups({
			databasePath,
			settings: s3.settings,
			logger: silentLogger,
			startedAt: new Date('2026-10-28T12:00:00Z')
		});

		// The bucket doesn't answer, so the app can't tell whether it holds any.
		await backups.runIfDue(new Date('2026-10-28T12:00:00Z'));
		expect(backups.status(new Date('2026-10-30T11:59:00Z'))).toMatchObject({ latestAt: null, stale: false });
		expect(backups.status(new Date('2026-10-30T12:01:00Z'))).toMatchObject({ latestAt: null, stale: true });
		expect(backups.status().lastAttempt?.error).toBeTruthy();
		backups.close();
		s3 = await fakeS3();
	});
});
