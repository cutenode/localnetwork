// Backs up the database to the bucket, lists the backups, or restores one. Plain Node runs it (it
// strips the types itself), here and in the production image, where `railway ssh` reaches it:
//   node scripts/db-backup.ts list
//   node scripts/db-backup.ts backup
//   node scripts/db-backup.ts restore <key|latest> [path]
// The bucket comes from the BACKUP_S3_* variables (.env.example), the database from DATABASE_PATH.
import { existsSync } from 'node:fs';
import { listBackups, restoreBackup, takeBackup } from '../src/lib/server/backup/backups.ts';
import { DEFAULT_DATABASE_PATH, loadBackupSettings } from '../src/lib/server/backup/config.ts';
import { createStore } from '../src/lib/server/backup/store.ts';
import { createLogger } from '../src/lib/server/logger.ts';

const USAGE = 'usage: db-backup.ts list | backup | restore <key|latest> [path]';

if (process.env.NODE_ENV !== 'production') {
	for (const file of ['.env.local', '.env']) {
		if (existsSync(file)) process.loadEnvFile(file);
	}
}

const [command, ...args] = process.argv.slice(2);

const settings = loadBackupSettings(process.env);

if (!settings) {
	console.error('Set BACKUP_S3_BUCKET and the other BACKUP_S3_* variables (see .env.example).');
	process.exit(1);
}

const databasePath = process.env.DATABASE_PATH || DEFAULT_DATABASE_PATH;

const logger = createLogger({ level: process.env.LOG_LEVEL ?? 'info', pretty: process.env.NODE_ENV !== 'production' });

const store = createStore(settings);

try {
	if (command === 'list') {
		for (const stored of await listBackups(store)) console.log(`${stored.takenAt.toISOString()}  ${stored.key}`);
	} else if (command === 'backup') {
		await takeBackup({ databasePath, store, keep: settings.keep, logger });
	} else if (command === 'restore' && args[0]) {
		const key = args[0] === 'latest' ? (await listBackups(store))[0]?.key : args[0];

		if (!key) throw new Error('the bucket has no backups yet');
		await restoreBackup({ key, targetPath: args[1] ?? databasePath, store, logger });
	} else {
		console.error(USAGE);
		process.exitCode = 2;
	}
} catch (error) {
	logger.error({ err: error }, `db-backup ${command} failed`);
	process.exitCode = 1;
} finally {
	store.close();
}
