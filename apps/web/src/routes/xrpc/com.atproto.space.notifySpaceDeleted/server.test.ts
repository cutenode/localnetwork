import { Secp256k1Keypair } from '@atproto/crypto';
import type { JsonObject } from '@atproto/lex-json';
import * as fc from 'fast-check';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { parseSpaceUri } from '#lib/server/atproto/spaces.ts';
import { parseJson } from '#lib/server/db/index.ts';
import { remoteObject } from '#lib/server/testing/remote-bodies.ts';
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

	const space = fc.oneof(
		fc.constantFrom(
			SPACE,
			`at://${AUTHORITY}/space/computer.localnetwork.space.party/other`,
			'at://did:plc:else/space/a.b.c/x'
		),
		fc.jsonValue()
	);

	/** The body's `space`, when it has a string there. */
	const namedSpace = z.object({ space: z.string() });

	/** Claims a token is minted with: the authority's own, or one that should be refused. */
	const claims = fc.constantFrom<JsonObject | null>(
		{},
		{ iss: 'did:plc:someone-else' },
		{ aud: 'did:web:elsewhere.test' },
		{ lxm: 'com.atproto.space.notifyWrite' },
		{ exp: NOW - 1 },
		null
	);

	it('forgets a space only for its authority, and answers a 4xx to everything else', async () => {
		await fc.assert(
			fc.asyncProperty(
				remoteObject({ space }),
				claims,
				fc.string({ unit: 'grapheme-ascii' }),
				async (body, changed, garbage) => {
					const authorization = changed ? await token({ exp: NOW + 60, ...changed }) : garbage;
					const before = syncer.deleted.length;
					const { status } = await notify(body, authorization);
					const named = namedSpace.safeParse(parseJson<unknown>(body, null));
					const ref = named.success ? parseSpaceUri(named.data.space) : null;
					const accepted = ref?.authority === AUTHORITY && changed !== null && Object.keys(changed).length === 0;

					expect(accepted ? [200] : [400, 401]).toContain(status);
					expect(syncer.deleted.length - before).toBe(accepted ? 1 : 0);
				}
			),
			{ numRuns: 50 }
		);
	});
});
