import { expect, it } from 'vitest';
import { loadBackupSettings } from './config';

const bucket = {
	BACKUP_S3_BUCKET: 'localnetwork-backups',
	BACKUP_S3_ENDPOINT: 'https://s3.example',
	BACKUP_S3_ACCESS_KEY_ID: 'key',
	BACKUP_S3_SECRET_ACCESS_KEY: 'secret'
};

it('reads the backup settings alone, with their defaults, and none without a bucket', () => {
	expect(loadBackupSettings({})).toBeUndefined();
	expect(loadBackupSettings(bucket)).toEqual({
		endpoint: 'https://s3.example',
		region: 'auto',
		bucket: 'localnetwork-backups',
		accessKeyId: 'key',
		secretAccessKey: 'secret',
		pathStyle: false,
		keep: { daily: 7, weekly: 8 }
	});
});

it('says which backup settings are wrong', () => {
	expect(() => loadBackupSettings({ ...bucket, BACKUP_KEEP_DAILY: '0' })).toThrow(
		/^Invalid backup configuration:\n {2}BACKUP_KEEP_DAILY: /
	);
});
