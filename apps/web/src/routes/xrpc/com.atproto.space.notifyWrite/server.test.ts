import { Secp256k1Keypair } from '@atproto/crypto';
import type { JsonObject } from '@atproto/lex-json';
import * as fc from 'fast-check';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { type Db, nowIso, openDatabase, parseJson } from '#lib/server/db/index.ts';
import { remoteObject } from '#lib/server/testing/remote-bodies.ts';
import { serviceJwt } from '#lib/server/testing/service-jwt.ts';
import { type WrittenLine, writtenLog } from '#lib/server/testing/written-log.ts';
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

/** What the endpoint logged. */
let logged: WrittenLine[];

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
		log: writtenLog('warn', logged).logger
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
	expect(logged.map((line) => line.message)).toEqual(['notified sync failed']);
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

describe('for any body and Authorization header', () => {
	// Fixed time, so tokens expire against the same clock every run.
	const NOW = 1_800_000_000;

	beforeEach(() => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(NOW * 1000);
	});

	afterEach(() => {
		vi.useRealTimers();
	});

	// Mostly real space URIs, so a good token for a space nobody watches comes up in most runs.
	const space = fc.oneof(
		{ arbitrary: fc.constant(SPACE), weight: 3 },
		{
			arbitrary: fc.constantFrom(
				`at://${AUTHORITY}/space/computer.localnetwork.space.booking/other`,
				'at://did:plc:else/space/a.b.c/x'
			),
			weight: 2
		},
		{ arbitrary: fc.jsonValue(), weight: 1 }
	);

	const body = fc.oneof(
		{ arbitrary: space.map((value) => JSON.stringify({ space: value })), weight: 3 },
		{ arbitrary: remoteObject({ space }), weight: 1 }
	);

	/** Claims a token is minted with: the authority's own, another issuer's otherwise valid one, or one that's wrong. */
	const claims = fc.oneof(
		{ arbitrary: fc.constant<JsonObject | null>({}), weight: 4 },
		{
			arbitrary: fc
				.constantFrom('did:plc:someone-else', 'did:web:elsewhere.test')
				.map<JsonObject | null>((iss) => ({ iss })),
			weight: 3
		},
		{
			arbitrary: fc.constantFrom<JsonObject | null>(
				{ aud: 'did:web:elsewhere.test' },
				{ lxm: 'com.atproto.space.notifySpaceDeleted' },
				{ exp: NOW - 1 },
				null
			),
			weight: 4
		}
	);

	it('syncs only for the authority of a watched space, and answers a 4xx to everything else', async () => {
		await fc.assert(
			fc.asyncProperty(body, claims, fc.string({ unit: 'grapheme-ascii' }), async (sent, changed, garbage) => {
				const authorization = changed ? await token({ exp: NOW + 60, ...changed }) : garbage;
				const before = syncer.synced.length;
				const { status } = await notify(sent, authorization);
				const named = parseJson<{ space?: unknown } | null>(sent, null)?.space;
				const accepted = named === SPACE && changed !== null && Object.keys(changed).length === 0;

				expect(accepted ? [200] : [400, 401, 404]).toContain(status);
				expect(syncer.synced.length - before).toBe(accepted ? 1 : 0);
			}),
			{ numRuns: 100 }
		);
	});
});
