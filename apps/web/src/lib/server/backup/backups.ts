import { createReadStream, createWriteStream, existsSync, mkdirSync } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { backup, DatabaseSync } from 'node:sqlite';
import { pipeline } from 'node:stream/promises';
import { createGunzip, createGzip } from 'node:zlib';
import type { Logger } from '../logger.ts';
import type { BackupSettings } from './config.ts';
import { expiredBackups, isBackupDue, type RetentionPolicy, type StoredBackup } from './retention.ts';
import { type BackupStore, createStore } from './store.ts';

const PREFIX = 'localnetwork-db/';

// The time it was taken, as an ISO timestamp without its separators: localnetwork-db/20261030T001500123Z.sqlite.gz.
const KEY = /^localnetwork-db\/(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})(\d{3})Z\.sqlite\.gz$/;

export const backupKey = (takenAt: Date) => `${PREFIX}${takenAt.toISOString().replace(/[-:.]/g, '')}.sqlite.gz`;

function parseKey(key: string): StoredBackup | undefined {
	const parts = KEY.exec(key);

	if (!parts) return undefined;
	const [, year, month, day, hour, minute, second, ms] = parts;

	return { key, takenAt: new Date(`${year}-${month}-${day}T${hour}:${minute}:${second}.${ms}Z`) };
}

/** The bucket's backups, newest first. Anything else in the bucket is left out, so it's never deleted. */
export async function listBackups(store: BackupStore): Promise<StoredBackup[]> {
	const backups = (await store.list(PREFIX)).map(parseKey).filter((parsed) => parsed !== undefined);

	return backups.sort((a, b) => b.takenAt.getTime() - a.takenAt.getTime());
}

function openDatabase(path: string): DatabaseSync {
	const db = new DatabaseSync(path);
	db.exec('PRAGMA busy_timeout = 5000');

	return db;
}

/** Throws unless the file is a whole, uncorrupted SQLite database. */
function checkIntegrity(path: string) {
	const db = new DatabaseSync(path);

	try {
		const rows = db.prepare('PRAGMA integrity_check').all();
		const problems = rows.map((row) => String(row.integrity_check)).filter((result) => result !== 'ok');

		if (problems.length > 0) throw new Error(`${path} is damaged: ${problems.slice(0, 5).join('; ')}`);
	} finally {
		db.close();
	}
}

// Pages per backup step: all of them (Node takes no -1, SQLite's "all"), since a write from the app
// between steps would start the copy over. In WAL mode one step holds a read lock, which doesn't
// stop the app writing, and Node runs it off the main thread.
const ALL_PAGES = 2 ** 31 - 1;

/**
 * Copies one database into another with SQLite's online backup, which gives a consistent copy while
 * the app keeps reading and writing (unlike copying the file and its WAL).
 */
async function copyDatabase(from: string, to: string) {
	const source = openDatabase(from);

	try {
		await backup(source, to, { rate: ALL_PAGES });
	} finally {
		source.close();
	}
}

async function withScratch<T>(task: (dir: string) => Promise<T>): Promise<T> {
	const dir = await mkdtemp(join(tmpdir(), 'localnetwork-backup-'));

	try {
		return await task(dir);
	} finally {
		await rm(dir, { recursive: true, force: true });
	}
}

type BackupOptions = { databasePath: string; store: BackupStore; keep: RetentionPolicy; logger: Logger; now?: Date };

/** Snapshots the database, uploads it gzipped, then deletes the backups the retention policy no longer keeps. */
export async function takeBackup({ databasePath, store, keep, logger, now = new Date() }: BackupOptions) {
	if (!existsSync(databasePath)) throw new Error(`${databasePath} doesn't exist, so there's nothing to back up`);
	const started = performance.now();
	const key = backupKey(now);

	const bytes = await withScratch(async (dir) => {
		const snapshot = join(dir, 'snapshot.sqlite');
		await copyDatabase(databasePath, snapshot);
		checkIntegrity(snapshot);
		const gzipped = `${snapshot}.gz`;
		await pipeline(createReadStream(snapshot), createGzip(), createWriteStream(gzipped));

		return store.upload(key, gzipped);
	});

	const expired = expiredBackups(await listBackups(store), keep);

	for (const old of expired) await store.delete(old.key);
	logger.info(
		{ key, bytes, ms: Math.round(performance.now() - started), deleted: expired.map((old) => old.key) },
		'backed up the database'
	);

	return { key, bytes, deleted: expired.map((old) => old.key) };
}

type RestoreOptions = { key: string; targetPath: string; store: BackupStore; logger: Logger };

/**
 * Writes a backup over the database at `targetPath`, with the same online backup, so it's safe while
 * the app has the database open. The app keeps some state in memory, so restart it afterwards. The
 * database it replaces is kept beside it as `<name>.before-restore`.
 */
export async function restoreBackup({ key, targetPath, store, logger }: RestoreOptions) {
	if (!parseKey(key)) throw new Error(`${key} isn't a backup's key (\`list\` shows them)`);

	await withScratch(async (dir) => {
		const gzipped = join(dir, 'snapshot.sqlite.gz');
		const snapshot = join(dir, 'snapshot.sqlite');
		await store.download(key, gzipped);
		await pipeline(createReadStream(gzipped), createGunzip(), createWriteStream(snapshot));
		checkIntegrity(snapshot);

		const previous = existsSync(targetPath) ? `${targetPath}.before-restore` : null;

		if (previous) await copyDatabase(targetPath, previous);
		else mkdirSync(dirname(targetPath), { recursive: true });
		await copyDatabase(snapshot, targetPath);
		logger.info({ key, targetPath, previous }, 'restored the database');
	});
}

type ScheduleOptions = { databasePath: string; settings: BackupSettings; logger: Logger; startedAt?: Date };

/** How the app's backups are going, for the System page and /healthz. */
export type BackupStatus = {
	/** When the newest backup was taken, as far as this process knows (the bucket's listing, then its own). */
	latestAt: string | null;
	/** The last backup this process tried, and why it failed. Null until it has tried one. */
	lastAttempt: { at: string; error: string | null } | null;
	/** No backup in the last two days (counted from when the app started, while it knows of none). */
	stale: boolean;
};

const STALE_AFTER_MS = 2 * 86_400_000;

/**
 * The app's daily backup: `runIfDue` backs up once per UTC day, on its first run that day. It lists
 * the bucket once, then remembers when it last backed up. A failure is logged and kept for
 * `status()` rather than thrown, and the next run tries again.
 */
export function dailyBackups({ databasePath, settings, logger, startedAt = new Date() }: ScheduleOptions) {
	const store = createStore(settings);
	let latest: Date | undefined;
	let listed = false;
	let lastAttempt: BackupStatus['lastAttempt'] = null;

	return {
		async runIfDue(now = new Date()) {
			try {
				if (!listed) {
					latest = (await listBackups(store))[0]?.takenAt;
					listed = true;
				}

				if (!isBackupDue(latest, now)) return undefined;
				const result = await takeBackup({ databasePath, store, keep: settings.keep, logger, now });
				latest = now;
				lastAttempt = { at: now.toISOString(), error: null };

				return result;
			} catch (error) {
				logger.error({ err: error }, 'could not back up the database');
				const message = error instanceof Error ? error.message : String(error);
				lastAttempt = { at: now.toISOString(), error: message.slice(0, 200) };

				return undefined;
			}
		},

		status(now = new Date()): BackupStatus {
			const since = latest ?? startedAt;

			return {
				latestAt: latest?.toISOString() ?? null,
				lastAttempt,
				stale: now.getTime() - since.getTime() > STALE_AFTER_MS
			};
		},

		close: () => store.close()
	};
}
