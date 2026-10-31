import { createReadStream, createWriteStream } from 'node:fs';
import { stat } from 'node:fs/promises';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import {
	DeleteObjectCommand,
	GetObjectCommand,
	ListObjectsV2Command,
	PutObjectCommand,
	S3Client
} from '@aws-sdk/client-s3';
import type { BackupSettings } from './config.ts';

export type BackupStore = ReturnType<typeof createStore>;

/** The bucket, through the S3 API: just the four calls backups need. */
export function createStore(settings: BackupSettings) {
	const client = new S3Client({
		endpoint: settings.endpoint,
		region: settings.region,
		forcePathStyle: settings.pathStyle,
		credentials: { accessKeyId: settings.accessKeyId, secretAccessKey: settings.secretAccessKey },
		// Checksums only where S3 requires them: S3-compatible stores don't all take the SDK's default ones.
		requestChecksumCalculation: 'WHEN_REQUIRED',
		responseChecksumValidation: 'WHEN_REQUIRED'
	});

	const Bucket = settings.bucket;

	return {
		async list(prefix: string): Promise<string[]> {
			const keys: string[] = [];
			let ContinuationToken: string | undefined;

			do {
				const page = await client.send(new ListObjectsV2Command({ Bucket, Prefix: prefix, ContinuationToken }));

				for (const object of page.Contents ?? []) if (object.Key) keys.push(object.Key);
				ContinuationToken = page.IsTruncated ? page.NextContinuationToken : undefined;
			} while (ContinuationToken);

			return keys;
		},

		async upload(key: string, path: string): Promise<number> {
			const { size } = await stat(path);
			await client.send(new PutObjectCommand({ Bucket, Key: key, Body: createReadStream(path), ContentLength: size }));

			return size;
		},

		async download(key: string, path: string): Promise<void> {
			const object = await client.send(new GetObjectCommand({ Bucket, Key: key }));

			if (!(object.Body instanceof Readable)) throw new Error(`${key}: the bucket sent no body`);
			await pipeline(object.Body, createWriteStream(path));
		},

		async delete(key: string): Promise<void> {
			await client.send(new DeleteObjectCommand({ Bucket, Key: key }));
		},

		close() {
			client.destroy();
		}
	};
}
