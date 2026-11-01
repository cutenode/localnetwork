import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { describe, expect, it, vi } from 'vitest';
import { IdentityResolver } from '../atproto/identity';
import { silentLogger } from '../logger';
import { type AudioRequest, parseRange, recordingAudio } from './audio';

const DJ = 'did:plc:dj';

const CID = 'bafkreigtpuhqv5ggwnsphu54sjzurjztcspl4xp2t7wcaudioaudioaudio';

/** The recording's bytes: 0, 1, 2, ... 99. */
const BYTES = Uint8Array.from({ length: 100 }, (_, i) => i);

/**
 * A PDS that serves BYTES for CID in chunks of `chunk` bytes, ignoring Range as PDSes do, and records each request and
 * how many chunks it was asked for before the reader went away.
 */
function pds({ chunk = 7, status = 200, bytes = BYTES } = {}) {
	const requests: { url: string; range: string | null; pulled: number; cancelled: boolean }[] = [];

	const fetch: typeof globalThis.fetch = async (input, init) => {
		const request = new Request(input, init);
		const seen = { url: request.url, range: request.headers.get('range'), pulled: 0, cancelled: false };
		requests.push(seen);

		if (status !== 200) return new Response('nope', { status });

		let offset = 0;

		const body = new ReadableStream<Uint8Array>(
			{
				pull(controller) {
					if (offset >= bytes.length) return controller.close();
					seen.pulled++;
					controller.enqueue(bytes.slice(offset, offset + chunk));
					offset += chunk;
				},
				cancel() {
					seen.cancelled = true;
				}
			},
			// Read only when asked, so `pulled` counts what the route read.
			{ highWaterMark: 0 }
		);

		return new Response(body, { headers: { 'content-type': 'application/octet-stream' } });
	};

	return { fetch, requests };
}

const declared = { mimeType: 'audio/mpeg', size: BYTES.length };

/**
 * The DJ's identity as the route has it: a DID document naming the PDS at `endpoint` (or failing with it), reached
 * through `network` with the guarded fetch, private addresses allowed or not.
 */
function identityOf(endpoint: string | Error, network: typeof globalThis.fetch, allowPrivateNetwork: boolean) {
	const identity = new IdentityResolver({
		plcUrl: 'https://plc.test',
		allowPrivateNetwork,
		logger: silentLogger,
		fetch: network
	});

	vi.spyOn(identity, 'resolve').mockImplementation(async () => {
		if (endpoint instanceof Error) throw endpoint;

		return { did: DJ, handle: null, pds: endpoint, signingKey: 'did:key:zDJ', kemKey: null };
	});

	return identity;
}

/** What a request can change: the recording, and the PDS the DJ's DID document names, how it's reached and checked. */
type Overrides = Partial<Omit<AudioRequest, 'identity'>> & {
	endpoint?: string | Error;
	fetch?: typeof globalThis.fetch;
	allowPrivateNetwork?: boolean;
};

function ask(range: string | null, overrides: Overrides = {}) {
	const { endpoint = 'https://pds.example', fetch = pds().fetch, allowPrivateNetwork = true, ...request } = overrides;

	return recordingAudio({
		did: DJ,
		cid: CID,
		range,
		audio: async (did, cid) => (did === DJ && cid === CID ? declared : null),
		identity: identityOf(endpoint, fetch, allowPrivateNetwork),
		log: silentLogger,
		...request
	});
}

const body = async (response: Response) => [...new Uint8Array(await response.arrayBuffer())];

const range = (start: number, end: number) => Array.from({ length: end - start + 1 }, (_, i) => start + i);

describe('parseRange', () => {
	it('reads a start, an open end and a suffix against the size', () => {
		expect(parseRange('bytes=10-19', 100)).toEqual({ start: 10, end: 19 });
		expect(parseRange('bytes=90-', 100)).toEqual({ start: 90, end: 99 });
		expect(parseRange('bytes=90-500', 100)).toEqual({ start: 90, end: 99 });
		expect(parseRange('bytes=-10', 100)).toEqual({ start: 90, end: 99 });
		expect(parseRange('bytes=-500', 100)).toEqual({ start: 0, end: 99 });
	});

	it('sends the whole file for no range, another unit, nonsense or several ranges', () => {
		for (const header of [null, '', 'items=0-1', 'bytes=-', 'bytes=a-b', 'bytes=20-10', 'bytes=0-1,5-9'])
			expect(parseRange(header, 100)).toBeNull();
	});

	it("can't satisfy a range past the end, or an empty suffix", () => {
		expect(parseRange('bytes=100-', 100)).toBe('unsatisfiable');
		expect(parseRange('bytes=-0', 100)).toBe('unsatisfiable');
	});
});

describe('recordingAudio', () => {
	it('sends the whole recording as its record declares it, with headers for immutable, locked-down audio', async () => {
		const response = await ask(null);

		expect(response.status).toBe(200);
		expect(await body(response)).toEqual([...BYTES]);
		expect(Object.fromEntries(response.headers)).toMatchObject({
			'content-type': 'audio/mpeg',
			'content-length': '100',
			'accept-ranges': 'bytes',
			etag: `"${CID}"`,
			'cache-control': 'public, max-age=31536000, immutable',
			'x-content-type-options': 'nosniff'
		});
		expect(response.headers.get('content-security-policy')).toContain("default-src 'none'");
		expect(response.headers.get('content-security-policy')).toBe(
			"default-src 'none'; media-src 'self'; style-src 'unsafe-inline'"
		);
		expect(response.headers.has('content-range')).toBe(false);
	});

	it("refuses a recording whose declared type the lexicon doesn't take, and stops reading the PDS", async () => {
		const upstream = pds();

		const response = await ask(null, {
			audio: async () => ({ mimeType: 'text/html', size: BYTES.length }),
			fetch: upstream.fetch
		});

		expect(response.status).toBe(404);
		expect(upstream.requests.map(({ pulled, cancelled }) => ({ pulled, cancelled }))).toEqual([
			{ pulled: 0, cancelled: true }
		]);
	});

	it("answers a range from the PDS's whole file, across its chunks", async () => {
		for (const [header, start, end] of [
			['bytes=0-0', 0, 0],
			['bytes=5-30', 5, 30],
			['bytes=14-20', 14, 20],
			['bytes=93-', 93, 99],
			['bytes=-12', 88, 99]
		] as const) {
			const response = await ask(header);

			expect(response.status).toBe(206);
			expect(response.headers.get('content-range')).toBe(`bytes ${start}-${end}/100`);
			expect(response.headers.get('content-length')).toBe(String(end - start + 1));
			expect(await body(response)).toEqual(range(start, end));
		}
	});

	it('stops reading the PDS once the range is out, and stores nothing between requests', async () => {
		const upstream = pds({ chunk: 10 });

		expect(await body(await ask('bytes=12-15', { fetch: upstream.fetch }))).toEqual(range(12, 15));
		expect(await body(await ask('bytes=12-15', { fetch: upstream.fetch }))).toEqual(range(12, 15));

		expect(upstream.requests).toHaveLength(2);
		expect(upstream.requests.map(({ pulled, cancelled }) => ({ pulled, cancelled }))).toEqual([
			{ pulled: 2, cancelled: true },
			{ pulled: 2, cancelled: true }
		]);
		// Checks the route reads the blob from the PDS, which its own ast-grep-ignore allows.
		// ast-grep-ignore: no-direct-pds-read
		expect(new URL(upstream.requests[0].url).pathname).toBe('/xrpc/com.atproto.sync.getBlob');
		expect(Object.fromEntries(new URL(upstream.requests[0].url).searchParams)).toEqual({ did: DJ, cid: CID });
	});

	it('never sends more than the record declares, whatever the PDS has', async () => {
		const response = await ask(null, { fetch: pds({ bytes: Uint8Array.from({ length: 150 }, (_, i) => i) }).fetch });

		expect(await body(response)).toEqual([...BYTES]);
	});

	it('fails the body when the PDS sends less than the record declares', async () => {
		const response = await ask(null, { fetch: pds({ bytes: BYTES.slice(0, 50) }).fetch });

		await expect(response.arrayBuffer()).rejects.toThrow('the PDS sent less than the recording declares');
	});

	it('answers 416 for a range past the end, without asking the PDS', async () => {
		const upstream = pds();
		const response = await ask('bytes=100-', { fetch: upstream.fetch });

		expect(response.status).toBe(416);
		expect(response.headers.get('content-range')).toBe('bytes */100');
		expect(upstream.requests).toHaveLength(0);
	});

	it("refuses a CID no recording references (or the site doesn't serve), without asking the PDS", async () => {
		const upstream = pds();
		const response = await ask(null, { cid: 'bafyunreferenced', fetch: upstream.fetch });

		expect(response.status).toBe(404);
		expect(upstream.requests).toHaveLength(0);
	});

	it("passes on a PDS that doesn't have the blob, and fails one that's down", async () => {
		expect((await ask(null, { fetch: pds({ status: 404 }).fetch })).status).toBe(404);
		expect((await ask(null, { fetch: pds({ status: 500 }).fetch })).status).toBe(502);

		const unreachable = await ask(null, { endpoint: new Error('no PDS in the DID document') });

		expect(unreachable.status).toBe(502);
	});

	it('refuses a recording whose DID points at a private address or plain HTTP, without asking it', async () => {
		for (const endpoint of [
			'https://127.0.0.1',
			'https://10.0.0.1',
			'https://169.254.169.254',
			'https://[::1]',
			'http://pds.example'
		]) {
			const upstream = pds();
			const response = await ask(null, { endpoint, fetch: upstream.fetch, allowPrivateNetwork: false });

			expect(response.status).toBe(502);
			expect(upstream.requests).toHaveLength(0);
		}
	});

	it('still serves ranges from a public PDS', async () => {
		const upstream = pds();

		const response = await ask('bytes=5-30', {
			endpoint: 'https://8.8.8.8',
			fetch: upstream.fetch,
			allowPrivateNetwork: false
		});

		expect(response.status).toBe(206);
		expect(await body(response)).toEqual(range(5, 30));
		expect(new URL(upstream.requests[0].url).origin).toBe('https://8.8.8.8');
	});

	it("doesn't follow a PDS that redirects", async () => {
		let followed = false;

		const server = createServer((request, response) => {
			if (request.url?.startsWith('/internal')) {
				followed = true;
				response.end(Buffer.from(BYTES));

				return;
			}

			response.writeHead(302, { location: '/internal' }).end();
		});

		await new Promise<void>((listening) => server.listen(0, '127.0.0.1', listening));

		try {
			// SAFETY: a server listening on a TCP port has an address and port, not a pipe name.
			const { port } = server.address() as AddressInfo;
			const response = await ask(null, { endpoint: `http://127.0.0.1:${port}`, fetch: globalThis.fetch });

			expect(response.status).toBe(502);
			expect(followed).toBe(false);
		} finally {
			await new Promise((closed) => server.close(closed));
		}
	});
});
