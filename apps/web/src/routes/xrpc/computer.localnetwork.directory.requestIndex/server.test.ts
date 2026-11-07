import { Secp256k1Keypair } from '@atproto/crypto';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { REQUEST_INDEX } from '#lib/nsid.ts';
import { Backfills } from '#lib/server/happyview/backfills.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { serviceJwt } from '#lib/server/testing/service-jwt.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { POST } from './+server.ts';

const SERVICE_DID = 'did:web:localnetwork.test';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

let key: Secp256k1Keypair;

beforeEach(async () => {
	t = await createTestServices();
	key = await Secp256k1Keypair.create();
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** locallibrary asks for DJ's records to be read again, with `authorization`, and gets the status and answer. */
async function requestIndex(authorization: string) {
	const response = await callRoute(POST, {
		app: {
			config: { serviceDid: SERVICE_DID },
			identity: { signingKey: async () => key.did() },
			backfills: new Backfills(t.directory, silentLogger)
		},
		request: new Request(`https://localnetwork.test/xrpc/${REQUEST_INDEX}`, {
			method: 'POST',
			headers: { authorization }
		})
	});

	return { status: response.status, body: await response.json() };
}

it("has HappyView re-read the caller's records", async () => {
	const authorization = await serviceJwt(key, {
		iss: DJ,
		aud: SERVICE_DID,
		lxm: REQUEST_INDEX,
		exp: Math.floor(Date.now() / 1000) + 60
	});

	expect(await requestIndex(authorization)).toEqual({ status: 200, body: { indexed: true } });
});

it('refuses a token minted for another service', async () => {
	const authorization = await serviceJwt(key, {
		iss: DJ,
		aud: 'did:web:elsewhere.test',
		lxm: REQUEST_INDEX,
		exp: Math.floor(Date.now() / 1000) + 60
	});

	expect(await requestIndex(authorization)).toMatchObject({ status: 401, body: { error: 'AuthenticationRequired' } });
});
