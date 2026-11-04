import { Secp256k1Keypair } from '@atproto/crypto';
import type { JsonObject } from '@atproto/lex-json';
import { pino } from 'pino';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { type Db, nowIso, openDatabase } from '#lib/server/db/index.ts';
import { serviceJwt } from '#lib/server/testing/service-jwt.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { POST } from './+server.ts';

const SYNC_SERVICE = 'did:web:localnetwork.test#localnetwork_syncer';

const AUTHORITY = 'did:plc:authority';

const SPACE = `at://${AUTHORITY}/space/computer.localnetwork.space.booking/self`;

const LXM = 'com.atproto.space.notifyWrite';

/** A space syncer that keeps which spaces it was asked to sync, and fails while `down`. */
class FakeSyncer {
	readonly synced: string[] = [];
	down = false;

	async sync(space: string): Promise<void> {
		if (this.down) throw new Error('space host unreachable');
		this.synced.push(space);
	}
}

let db: Db;

let key: Secp256k1Keypair;

let syncer: FakeSyncer;

/** What the endpoint logged, one JSON line each. */
let logged: string[];

beforeEach(async () => {
	db = await openDatabase(':memory:');
	key = await Secp256k1Keypair.create();
	syncer = new FakeSyncer();
	logged = [];
	await db
		.insertInto('spaceWatch')
		.values({
			spaceUri: SPACE,
			spaceType: 'computer.localnetwork.space.booking',
			authorityDid: AUTHORITY,
			skey: 'self',
			spaceRev: null,
			registrationExpiresAt: null,
			lastSyncedAt: null,
			lastError: null,
			createdAt: nowIso()
		})
		.execute();
});

afterEach(async () => {
	await db.destroy();
});

/** A service auth token from the space authority's PDS for this call, with `claims` changed. */
const token = (claims: JsonObject = {}) =>
	serviceJwt(key, { iss: AUTHORITY, aud: SYNC_SERVICE, lxm: LXM, exp: Math.floor(Date.now() / 1000) + 60, ...claims });

/** Posts `body` to the endpoint with `authorization`, and returns its status and answer once the sync it started ran. */
async function notify(body: string, authorization?: string) {
	const headers = new Headers({ 'content-type': 'application/json' });

	if (authorization) headers.set('authorization', authorization);

	const response = await callRoute(POST, {
		app: {
			db,
			config: { syncService: SYNC_SERVICE },
			identity: { signingKey: async () => key.did() },
			syncer
		},
		request: new Request(`https://localnetwork.test/xrpc/${LXM}`, { method: 'POST', headers, body }),
		log: pino({ level: 'warn' }, { write: (line: string) => void logged.push(line) })
	});

	await new Promise((resolve) => setImmediate(resolve));

	return { status: response.status, body: await response.json() };
}

it('syncs a watched space its authority says was written to', async () => {
	expect(await notify(JSON.stringify({ space: SPACE }), await token())).toEqual({ status: 200, body: {} });
	expect(syncer.synced).toEqual([SPACE]);
});

it('answers at once and logs the failure when the sync it started fails', async () => {
	syncer.down = true;

	expect(await notify(JSON.stringify({ space: SPACE }), await token())).toEqual({ status: 200, body: {} });
	expect(logged.map((line) => JSON.parse(line).msg)).toEqual(['notified sync failed']);
});

it("says so for a space this site doesn't watch", async () => {
	const other = `at://${AUTHORITY}/space/computer.localnetwork.space.booking/other`;

	expect(await notify(JSON.stringify({ space: other }), await token())).toEqual({
		status: 404,
		body: { error: 'SpaceNotFound' }
	});
	expect(syncer.synced).toEqual([]);
});

it('refuses a body that is not JSON, or names no space', async () => {
	expect(await notify('not json', await token())).toEqual({
		status: 400,
		body: { error: 'InvalidRequest', message: 'Expected JSON' }
	});
	expect(await notify(JSON.stringify({ uri: SPACE }), await token())).toEqual({
		status: 400,
		body: { error: 'InvalidRequest', message: 'Bad space' }
	});
	expect(syncer.synced).toEqual([]);
});

it('refuses a call without valid service auth from the space authority', async () => {
	const refused = { status: 401, body: { error: 'AuthenticationRequired', message: 'Invalid service auth' } };
	const body = JSON.stringify({ space: SPACE });

	expect(await notify(body)).toEqual(refused);
	expect(await notify(body, await token({ iss: 'did:plc:someone-else' }))).toEqual(refused);
	expect(await notify(body, await token({ aud: 'did:web:elsewhere.test' }))).toEqual(refused);
	expect(syncer.synced).toEqual([]);
});
