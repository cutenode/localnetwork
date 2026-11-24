import { Secp256k1Keypair } from '@atproto/crypto';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { REQUEST_INDEX } from '#lib/nsid.ts';
import { Backfills } from '#lib/server/happyview/backfills.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { serviceJwt } from '#lib/server/testing/service-jwt.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import type { Flags } from '#lib/types.ts';
import { POST } from './+server.ts';

const SERVICE_DID = 'did:web:localnetwork.test';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

let key: Secp256k1Keypair;

/** The backfills HappyView was asked to start. */
let backfills: string[];

beforeEach(async () => {
	t = await createTestServices();
	key = await Secp256k1Keypair.create();
	backfills = [];
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** locallibrary asks for DJ's records to be read again, with `authorization`, and gets the status and answer. */
async function requestIndex(authorization: string, flags?: Flags) {
	// The test's HappyView, watched for the backfills it's asked to start.
	const directory = new HappyViewClient({
		url: 'http://happyview.test',
		apiKey: 'hv_test',
		fetch: async (input, init) => {
			const request = new Request(input, init);

			if (new URL(request.url).pathname === '/admin/backfill') backfills.push(request.url);

			return t.happyView.fetch(request);
		}
	});

	const response = await callRoute(POST, {
		app: {
			config: { serviceDid: SERVICE_DID },
			identity: { signingKey: async () => key.did() },
			backfills: new Backfills(directory, silentLogger)
		},
		flags,
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
	expect(backfills).not.toEqual([]);
});

it('says nothing was indexed, starting no backfill, while pauseRequestIndex is on', async () => {
	const paused = await createTestServices({ flagOverrides: { pauseRequestIndex: true } });

	const authorization = await serviceJwt(key, {
		iss: DJ,
		aud: SERVICE_DID,
		lxm: REQUEST_INDEX,
		exp: Math.floor(Date.now() / 1000) + 60
	});

	expect(await requestIndex(authorization, await paused.flags.evaluate(null))).toEqual({
		status: 200,
		body: { indexed: false }
	});
	expect(backfills).toEqual([]);
	await paused.db.destroy();
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
