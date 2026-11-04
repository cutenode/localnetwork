import { Secp256k1Keypair } from '@atproto/crypto';
import type { JsonObject } from '@atproto/lex-json';
import { beforeEach, expect, it } from 'vitest';
import { serviceJwt } from '#lib/server/testing/service-jwt.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { POST } from './+server.ts';

const SYNC_SERVICE = 'did:web:localnetwork.test#localnetwork_syncer';

const AUTHORITY = 'did:plc:authority';

const SPACE = `at://${AUTHORITY}/space/computer.localnetwork.space.booking/self`;

const LXM = 'com.atproto.space.notifySpaceDeleted';

/** A space syncer that keeps which spaces it heard were deleted. */
class FakeSyncer {
	readonly deleted: string[] = [];

	async onSpaceDeleted(space: string): Promise<void> {
		this.deleted.push(space);
	}
}

let key: Secp256k1Keypair;

let syncer: FakeSyncer;

beforeEach(async () => {
	key = await Secp256k1Keypair.create();
	syncer = new FakeSyncer();
});

/** A service auth token from the space authority's PDS for this call, with `claims` changed. */
const token = (claims: JsonObject = {}) =>
	serviceJwt(key, { iss: AUTHORITY, aud: SYNC_SERVICE, lxm: LXM, exp: Math.floor(Date.now() / 1000) + 60, ...claims });

/** Posts `body` to the endpoint with `authorization`, and returns its status and answer. */
async function notify(body: string, authorization?: string) {
	const headers = new Headers({ 'content-type': 'application/json' });

	if (authorization) headers.set('authorization', authorization);

	const response = await callRoute(POST, {
		app: {
			config: { syncService: SYNC_SERVICE },
			identity: { signingKey: async () => key.did() },
			syncer
		},
		request: new Request(`https://localnetwork.test/xrpc/${LXM}`, { method: 'POST', headers, body })
	});

	return { status: response.status, body: await response.json() };
}

it('forgets a space its authority says was deleted', async () => {
	expect(await notify(JSON.stringify({ space: SPACE }), await token())).toEqual({ status: 200, body: {} });
	expect(syncer.deleted).toEqual([SPACE]);
});

it('refuses a body that names no space', async () => {
	const invalid = { status: 400, body: { error: 'InvalidRequest' } };

	expect(await notify('not json', await token())).toEqual(invalid);
	expect(await notify(JSON.stringify({ space: 'at://did:plc:authority/app.bsky.feed.post/1' }), await token())).toEqual(
		invalid
	);
	expect(syncer.deleted).toEqual([]);
});

it('refuses a call without valid service auth from the space authority', async () => {
	const refused = { status: 401, body: { error: 'AuthenticationRequired' } };
	const body = JSON.stringify({ space: SPACE });

	expect(await notify(body)).toEqual(refused);
	expect(await notify(body, await token({ iss: 'did:plc:someone-else' }))).toEqual(refused);
	expect(await notify(body, await token({ lxm: 'com.atproto.space.notifyWrite' }))).toEqual(refused);
	expect(syncer.deleted).toEqual([]);
});
