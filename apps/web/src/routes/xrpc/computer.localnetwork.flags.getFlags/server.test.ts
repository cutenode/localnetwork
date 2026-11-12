import { Secp256k1Keypair } from '@atproto/crypto';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { GET_FLAGS } from '#lib/nsid.ts';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { serviceJwt } from '#lib/server/testing/service-jwt.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

const SERVICE_DID = 'did:web:localnetwork.test';

const ADMIN = 'did:plc:admin';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

let key: Secp256k1Keypair;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
	key = await Secp256k1Keypair.create();
	await t.flags.updateFlag(await t.viewer(ADMIN), 'parityCanary', {
		enabled: true,
		everyone: false,
		percentage: 10,
		groups: [],
		dids: [DJ]
	});
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** Asks the route, with `authorization` if there is one, and gets what it answers about the one flag. */
async function ask(authorization?: string) {
	const response = await callRoute(GET, {
		app: {
			config: { serviceDid: SERVICE_DID },
			identity: { signingKey: async () => key.did() },
			flags: t.flags,
			limits: { flags: new RateLimiter(5, 1) }
		},
		request: new Request(`https://localnetwork.test/xrpc/${GET_FLAGS}`, {
			headers: authorization ? { authorization } : {}
		})
	});

	return response.json();
}

it('tells anyone the public gates, and targets nobody', async () => {
	expect(await ask()).toEqual({
		flags: [{ key: 'parityCanary', enabled: true, everyone: false, percentage: 10, targeted: false }]
	});
});

it('tells an account its own targeting, with a token from its server', async () => {
	const token = await serviceJwt(key, {
		iss: DJ,
		aud: SERVICE_DID,
		exp: Math.floor(Date.now() / 1000) + 60,
		lxm: GET_FLAGS
	});

	expect(await ask(token)).toEqual({
		flags: [{ key: 'parityCanary', enabled: true, everyone: false, percentage: 10, targeted: true }]
	});
});

it('says a flag is off for everyone, with the gates it keeps for when it is back on', async () => {
	await t.flags.updateFlag(await t.viewer(ADMIN), 'parityCanary', {
		enabled: false,
		everyone: false,
		percentage: 10,
		groups: [],
		dids: [DJ]
	});

	expect(await ask()).toEqual({
		flags: [{ key: 'parityCanary', enabled: false, everyone: false, percentage: 10, targeted: false }]
	});
});
