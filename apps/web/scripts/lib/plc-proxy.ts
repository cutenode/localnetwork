import { once } from 'node:events';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';

/** A did:plc DID's path, its 24-character id and what follows it. */
const PLC_PATH = /^\/did:plc:([a-z2-7]{24})(\/.*)?$/;

/**
 * What HappyView 3 reads after a DID on the PLC directory (src/profile.rs and src/plc.rs at v3.0.0-beta.14): its
 * document, its log and its last operation. It resolves a did:web from the DID's own domain, never from here.
 */
const ROUTES = ['', '/log', '/log/last'];

/** `path` on `base` when it's a PLC route, else undefined. */
function plcUrl(path: string, base: string): URL | undefined {
	const [, id, suffix = ''] = PLC_PATH.exec(path) ?? [];
	const route = ROUTES.find((known) => known === suffix);

	if (id === undefined || route === undefined) return undefined;
	// encodeURIComponent leaves a base32 id as it is. It's there so CodeQL's request-forgery check, which takes no
	// other check as proof, sees the id can't add a path or a host.
	const url = new URL(`/did:plc:${encodeURIComponent(id)}${route}`, base);

	return url.origin === new URL(base).origin ? url : undefined;
}

/**
 * A PLC directory for the dev HappyView, which reads every did:plc DID from the one PLC_URL it's given: the local
 * network's PLC for its own accounts, and plc.directory (`upstreamUrl`) for any other, such as those of the authorities
 * whose lexicons HappyView tracks from the network. Those, and the records it fetches from their PDSes, need the
 * internet. It only reads: anything but a GET of a PLC route gets a 400 or 405, so whatever reaches the proxy can't
 * pick another host or path to fetch, and an operation HappyView submits isn't passed on as a read.
 */
export async function startPlcProxy(
	localPlcUrl: string,
	upstreamUrl = 'https://plc.directory'
): Promise<{ url: string; close(): Promise<void> }> {
	const server = createServer((request, response) => {
		const path = request.url ?? '/';
		const local = plcUrl(path, localPlcUrl);
		const upstream = plcUrl(path, upstreamUrl);

		if (!local || !upstream) {
			response.writeHead(400, { 'content-type': 'text/plain' }).end('not a PLC route');

			return;
		}

		if (request.method !== 'GET') {
			response.writeHead(405, { allow: 'GET', 'content-type': 'text/plain' }).end('only reads pass through');

			return;
		}

		void (async () => {
			const found = await fetch(local);
			const answer = found.status === 404 ? await fetch(upstream) : found;

			response.writeHead(answer.status, { 'content-type': answer.headers.get('content-type') ?? 'text/plain' });
			response.end(Buffer.from(await answer.arrayBuffer()));
		})().catch((error: Error) => {
			response.writeHead(502).end(error.message);
		});
	}).listen(0, '127.0.0.1');

	await once(server, 'listening');
	// SAFETY: a server listening on a TCP port reports an AddressInfo.
	const { port } = server.address() as AddressInfo;

	return {
		url: `http://127.0.0.1:${port}`,
		close: () => new Promise((resolve) => server.close(() => resolve()))
	};
}
