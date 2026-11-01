import { z } from 'zod';
import type { RetentionPolicy } from './retention.ts';

/** Where the database lives unless DATABASE_PATH says otherwise. */
export const DEFAULT_DATABASE_PATH = 'data/localnetwork.db';

/** The S3-compatible bucket backups go to (a Railway bucket in production), and how many to keep. */
export type BackupSettings = {
	endpoint: string;
	region: string;
	bucket: string;
	accessKeyId: string;
	secretAccessKey: string;
	/** Older Railway buckets, and most local S3 stand-ins, only answer path-style URLs. */
	pathStyle: boolean;
	keep: RetentionPolicy;
};

const optional = z
	.string()
	.optional()
	.transform((value) => value || undefined);

const count = (fallback: number, min: number) => z.coerce.number().int().min(min).max(366).default(fallback);

/** The backup variables, merged into the app's own schema so every problem is reported at once. */
export const backupVariables = {
	BACKUP_S3_ENDPOINT: optional.pipe(z.url().optional()),
	BACKUP_S3_REGION: optional,
	BACKUP_S3_BUCKET: optional,
	BACKUP_S3_ACCESS_KEY_ID: optional,
	BACKUP_S3_SECRET_ACCESS_KEY: optional,
	BACKUP_S3_PATH_STYLE: z.enum(['true', 'false', '1', '0', 'yes', 'no']).optional(),
	// At least one daily backup, so the newest is always kept.
	BACKUP_KEEP_DAILY: count(7, 1),
	BACKUP_KEEP_WEEKLY: count(8, 0)
};

const backupEnv = z.object(backupVariables);

type BackupEnv = z.infer<typeof backupEnv>;

/** Backups are on once BACKUP_S3_BUCKET is set, and then need the endpoint and both keys too. */
export function backupSettings(value: BackupEnv): BackupSettings | undefined {
	if (!value.BACKUP_S3_BUCKET) return undefined;

	const missing = (['BACKUP_S3_ENDPOINT', 'BACKUP_S3_ACCESS_KEY_ID', 'BACKUP_S3_SECRET_ACCESS_KEY'] as const).filter(
		(name) => !value[name]
	);

	if (missing.length > 0) throw new Error(`BACKUP_S3_BUCKET is set, so set ${missing.join(', ')} too`);

	return {
		// SAFETY: `missing` above is empty, so the three are set.
		endpoint: value.BACKUP_S3_ENDPOINT!,
		region: value.BACKUP_S3_REGION ?? 'auto',
		bucket: value.BACKUP_S3_BUCKET,
		accessKeyId: value.BACKUP_S3_ACCESS_KEY_ID!,
		secretAccessKey: value.BACKUP_S3_SECRET_ACCESS_KEY!,
		pathStyle: ['true', '1', 'yes'].includes(value.BACKUP_S3_PATH_STYLE ?? ''),
		keep: { daily: value.BACKUP_KEEP_DAILY, weekly: value.BACKUP_KEEP_WEEKLY }
	};
}

/** Reads the backup settings alone, for the db:* scripts, which don't need the rest of the app's config. */
export function loadBackupSettings(env: Record<string, string | undefined>): BackupSettings | undefined {
	const parsed = backupEnv.safeParse(env);

	if (!parsed.success) {
		const problems = parsed.error.issues.map((issue) => `  ${issue.path.join('.')}: ${issue.message}`);
		throw new Error(`Invalid backup configuration:\n${problems.join('\n')}`);
	}

	return backupSettings(parsed.data);
}
