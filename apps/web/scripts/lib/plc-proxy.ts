import { once } from 'node:events';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';

/**
 * A PLC directory for the dev HappyView, which reads every DID from the one PLC_URL it's given: the local network's
 * PLC for its own accounts, and plc.directory for any other DID, such as those of the authorities whose lexicons
 * HappyView tracks from the network. Those, and the records it fetches from their PDSes, need the internet.
 */
export async function startPlcProxy(localPlcUrl: string): Promise<{ url: string; close(): Promise<void> }> {
	const server = createServer((request, response) => {
		const path = request.url ?? '/';

		void (async () => {
			const local = await fetch(`${localPlcUrl}${path}`);
			const upstream = local.status === 404 ? await fetch(`https://plc.directory${path}`) : local;

			response.writeHead(upstream.status, { 'content-type': upstream.headers.get('content-type') ?? 'text/plain' });
			response.end(Buffer.from(await upstream.arrayBuffer()));
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
