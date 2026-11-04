import { EventEmitter, once } from 'node:events';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';
import { buffer } from 'node:stream/consumers';
import type { BackupSettings } from '../backup/config.ts';

const escapeXml = (value: string) => value.replace(/[<>&]/g, (c) => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;' })[c]!);

/**
 * An in-memory bucket that answers the four S3 calls backups make (path-style), so tests run the
 * real S3 client without MinIO or Docker. `pageSize` makes listings page, as S3 does past 1000 keys,
 * and `silent` takes requests without ever answering them, for a client that hangs.
 */
export async function fakeS3({ bucket = 'backups', pageSize = 1000, silent = false } = {}) {
	const objects = new Map<string, Buffer>();
	const puts = new EventEmitter();

	const unanswered = new Set<IncomingMessage>();

	async function answer(request: IncomingMessage, response: ServerResponse) {
		const url = new URL(request.url ?? '/', 'http://s3.test');
		const [, name, ...rest] = url.pathname.split('/');
		const key = decodeURIComponent(rest.join('/'));

		if (name !== bucket) return void response.writeHead(404).end();

		if (request.method === 'GET' && !key) {
			const prefix = url.searchParams.get('prefix') ?? '';
			const start = Number(url.searchParams.get('continuation-token') ?? 0);
			const keys = [...objects.keys()].filter((candidate) => candidate.startsWith(prefix)).sort();
			const page = keys.slice(start, start + pageSize);

			const next =
				start + pageSize < keys.length ? `<NextContinuationToken>${start + pageSize}</NextContinuationToken>` : '';

			const contents = page
				.map((k) => `<Contents><Key>${escapeXml(k)}</Key><Size>${objects.get(k)!.length}</Size></Contents>`)
				.join('');

			return void response
				.writeHead(200, { 'content-type': 'application/xml' })
				.end(
					`<?xml version="1.0" encoding="UTF-8"?><ListBucketResult><Name>${bucket}</Name><KeyCount>${page.length}</KeyCount><IsTruncated>${next !== ''}</IsTruncated>${next}${contents}</ListBucketResult>`
				);
		}

		if (request.method === 'PUT') {
			objects.set(key, await buffer(request));
			puts.emit('put');

			return void response.writeHead(200).end();
		}

		if (request.method === 'GET') {
			const body = objects.get(key);

			if (!body) {
				return void response
					.writeHead(404, { 'content-type': 'application/xml' })
					.end('<Error><Code>NoSuchKey</Code><Message>The specified key does not exist.</Message></Error>');
			}

			return void response.writeHead(200, { 'content-length': body.length }).end(body);
		}

		if (request.method === 'DELETE') {
			objects.delete(key);

			return void response.writeHead(204).end();
		}

		response.writeHead(405).end();
	}

	const server = createServer((request, response) => {
		unanswered.add(request);
		response.on('finish', () => unanswered.delete(request));

		if (!silent) void answer(request, response);
	});

	server.listen(0, '127.0.0.1');
	await once(server, 'listening');
	// SAFETY: a server listening on a TCP port has an AddressInfo address.
	const { port } = server.address() as AddressInfo;

	const settings: BackupSettings = {
		endpoint: `http://127.0.0.1:${port}`,
		region: 'auto',
		bucket,
		accessKeyId: 'test',
		secretAccessKey: 'test',
		pathStyle: true,
		keep: { daily: 7, weekly: 8 }
	};

	return {
		objects,
		settings,
		/** The same bucket as BACKUP_S3_* variables, for the db-backup script. */
		env: {
			BACKUP_S3_ENDPOINT: settings.endpoint,
			BACKUP_S3_BUCKET: bucket,
			BACKUP_S3_ACCESS_KEY_ID: 'test',
			BACKUP_S3_SECRET_ACCESS_KEY: 'test',
			BACKUP_S3_PATH_STYLE: 'true'
		},
		/** Resolves once the bucket holds `count` objects, for uploads that run in the background. */
		stored: async (count: number) => {
			while (objects.size < count) await once(puts, 'put');
		},
		/** The requests taken and not answered yet, as `METHOD /path?query`: what a client is waiting on. */
		unanswered: () => [...unanswered].map(({ method, url }) => `${method} ${url}`),
		close: () => new Promise<void>((resolve) => server.close(() => resolve()))
	};
}
