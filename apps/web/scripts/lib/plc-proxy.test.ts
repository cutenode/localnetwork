import { once } from 'node:events';
import { createServer, request as send } from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterEach, describe, expect, it } from 'vitest';
import { startPlcProxy } from './plc-proxy.ts';

const LOCAL = 'did:plc:aaaaaaaaaaaaaaaaaaaaaaaa';

const REMOTE = 'did:plc:bbbbbbbbbbbbbbbbbbbbbbbb';

const closers: (() => Promise<void>)[] = [];

afterEach(async () => {
	await Promise.all(closers.splice(0).map((close) => close()));
});

/** A PLC directory that knows `known` DIDs and lists every path it was asked for in `asked`. */
async function directory(name: string, known: string[]) {
	const asked: string[] = [];

	const server = createServer((request, response) => {
		const path = request.url ?? '/';
		asked.push(path);
		const isKnown = known.some((did) => path.startsWith(`/${did}`));
		response.writeHead(isKnown ? 200 : 404, { 'content-type': 'text/plain' }).end(`${name} ${path}`);
	}).listen(0, '127.0.0.1');

	await once(server, 'listening');
	closers.push(() => new Promise((resolve) => server.close(() => resolve())));
	// SAFETY: a server listening on a TCP port reports an AddressInfo.
	const { port } = server.address() as AddressInfo;

	return { url: `http://127.0.0.1:${port}`, asked };
}

/** Sends `path` to `url` as written, the way a client that doesn't normalize it would. */
async function raw(url: string, path: string, method = 'GET'): Promise<{ status: number; body: string }> {
	const { hostname, port } = new URL(url);
	const [response] = await once(send({ hostname, port, path, method }).end(), 'response');
	let body = '';

	for await (const chunk of response) body += String(chunk);

	return { status: response.statusCode, body };
}

async function proxy() {
	const local = await directory('local', [LOCAL]);
	const remote = await directory('remote', [REMOTE]);
	const started = await startPlcProxy(local.url, remote.url);
	closers.push(() => started.close());

	return { url: started.url, local, remote };
}

describe('startPlcProxy', () => {
	it('answers a DID, its log and its last operation from the local PLC, and any other DID from upstream', async () => {
		const { url, local, remote } = await proxy();

		for (const suffix of ['', '/log', '/log/last'])
			expect(await raw(url, `/${LOCAL}${suffix}`)).toEqual({ status: 200, body: `local /${LOCAL}${suffix}` });

		expect(await raw(url, `/${REMOTE}/log/last`)).toEqual({ status: 200, body: `remote /${REMOTE}/log/last` });
		expect(local.asked).toHaveLength(4);
		expect(remote.asked).toEqual([`/${REMOTE}/log/last`]);
	});

	it('refuses anything but a PLC route without fetching it', async () => {
		const { url, local, remote } = await proxy();

		for (const path of [
			'/@evil.example/',
			'//evil.example/did:plc:aaaaaaaaaaaaaaaaaaaaaaaa',
			'/',
			'/did:plc:short',
			'/did:web:evil.example',
			`/${LOCAL}/log/other`,
			`/${LOCAL}/log/audit`,
			`/${LOCAL}/data`,
			`/${LOCAL}/../../admin`,
			`/${LOCAL}?url=http://evil.example`,
			`/${LOCAL.toUpperCase()}`
		])
			expect(await raw(url, path), path).toEqual({ status: 400, body: 'not a PLC route' });

		expect([...local.asked, ...remote.asked]).toEqual([]);
	});

	it('refuses an operation HappyView submits rather than passing it on as a read', async () => {
		const { url, local, remote } = await proxy();

		expect(await raw(url, `/${LOCAL}`, 'POST')).toEqual({ status: 405, body: 'only reads pass through' });
		expect([...local.asked, ...remote.asked]).toEqual([]);
	});
});
