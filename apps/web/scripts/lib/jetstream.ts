import { once } from 'node:events';
import type { AddressInfo } from 'node:net';
import { IdResolver } from '@atproto/identity';
import { lexStringify } from '@atproto/lex-json';
import { Firehose } from '@atproto/sync';
import { type WebSocket, WebSocketServer } from 'ws';

export type Jetstream = { url: string; close(): Promise<void> };

/**
 * A local stand-in for Jetstream (https://github.com/bluesky-social/jetstream): reads a PDS
 * firehose and serves its commits as Jetstream JSON on `/subscribe`, honoring `wantedCollections`
 * and `cursor` (replayed from an in-memory log). Records are written as lex JSON, so a consumer
 * that recomputes each CID from the JSON, as HappyView does, gets the CID the PDS committed.
 */
export async function startJetstream(options: { pdsUrl: string; plcUrl: string }): Promise<Jetstream> {
	const log: { timeUs: number; collection: string; message: string }[] = [];
	const subscribers = new Map<WebSocket, (collection: string) => boolean>();
	let lastTimeUs = 0;
	let lastSeq: number | undefined;
	const server = new WebSocketServer({ host: '127.0.0.1', port: 0, path: '/subscribe' });
	await once(server, 'listening');

	server.on('connection', (socket, request) => {
		const params = new URL(request.url ?? '/', 'ws://127.0.0.1').searchParams;
		const wanted = params.getAll('wantedCollections');
		const cursor = params.get('cursor');
		const matches = (collection: string) => wanted.length === 0 || wanted.includes(collection);

		// Without a cursor Jetstream streams live events only.
		if (cursor) {
			for (const event of log) {
				if (event.timeUs >= Number(cursor) && matches(event.collection)) socket.send(event.message);
			}
		}

		subscribers.set(socket, matches);
		socket.on('close', () => subscribers.delete(socket));
	});

	const firehose = new Firehose({
		service: options.pdsUrl.replace(/^http/, 'ws'),
		idResolver: new IdResolver({ plcUrl: options.plcUrl }),
		// The PDS is local, so commit signatures are not worth a key lookup per event.
		unauthenticatedCommits: true,
		excludeIdentity: true,
		excludeAccount: true,
		excludeSync: true,
		// Resumes after a dropped connection without losing commits.
		getCursor: () => lastSeq,
		handleEvent: (event) => {
			if (event.event !== 'create' && event.event !== 'update' && event.event !== 'delete') return;
			lastSeq = event.seq;
			// Jetstream cursors are unique, increasing microsecond timestamps.
			lastTimeUs = Math.max(Date.now() * 1000, lastTimeUs + 1);
			const { did, collection, rkey, rev } = event;

			const commit =
				event.event === 'delete'
					? { rev, operation: 'delete', collection, rkey }
					: { rev, operation: event.event, collection, rkey, record: event.record, cid: event.cid.toString() };

			const message = lexStringify({ did, time_us: lastTimeUs, kind: 'commit', commit });
			log.push({ timeUs: lastTimeUs, collection, message });

			for (const [socket, matches] of subscribers) {
				if (matches(collection)) socket.send(message);
			}
		},
		onError: (error) => console.warn(`Jetstream stand-in: ${error.message}`)
	});

	void firehose.start();
	// SAFETY: a server listening on a TCP port reports an AddressInfo.
	const { port } = server.address() as AddressInfo;

	return {
		url: `ws://127.0.0.1:${port}`,
		async close() {
			await firehose.destroy();

			for (const socket of server.clients) socket.terminate();
			await new Promise((resolve) => server.close(resolve));
		}
	};
}
