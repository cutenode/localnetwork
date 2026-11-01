import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { IdentityResolver } from '#lib/server/atproto/identity.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { type Loopback, rebindToLoopback } from '#lib/server/testing/rebind.ts';
import { GET } from './+server.ts';

const DJ = 'did:plc:dj';

const CID = 'bafkreigtpuhqv5ggwnsphu54sjzurjztcspl4xp2t7wcaudioaudioaudio';

const PDS = 'pds.rebind.localnetwork.computer';

let loopback: Loopback;

beforeEach(async () => {
	loopback = await rebindToLoopback([PDS]);
});

afterEach(async () => {
	vi.restoreAllMocks();
	await loopback.close();
});

it("won't stream a recording from a PDS whose name points at loopback when it connects, and never reaches it", async () => {
	const identity = new IdentityResolver({
		plcUrl: 'https://plc.test',
		allowPrivateNetwork: false,
		logger: silentLogger
	});

	vi.spyOn(identity, 'resolve').mockResolvedValue({
		did: DJ,
		handle: null,
		pds: `https://${PDS}:${loopback.port}`,
		signingKey: 'did:key:zDJ',
		kemKey: null
	});

	const recordings = { audio: async () => ({ mimeType: 'audio/mpeg', size: 100 }) };
	const locals = { app: { identity, recordings }, log: silentLogger };
	const request = new Request('https://x.test/');
	const event = { params: { did: DJ, cid: CID }, request, locals, fetch: globalThis.fetch };

	// SAFETY: the route reads only its params, the request's headers, the app's identity and recordings, and the log.
	const response = await GET(event as never);

	expect(response.status).toBe(502);
	expect(loopback.connections()).toBe(0);
});
