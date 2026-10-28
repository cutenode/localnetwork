import { Secp256k1Keypair } from '@atproto/crypto';
import type { JsonObject } from '@atproto/lex-json';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { COLLECTION, REQUEST_INDEX } from '#lib/nsid.ts';
import { lexicons } from '../atproto/lexicons';
import { silentLogger } from '../logger';
import { serviceJwt } from '../testing/service-jwt';
import { Backfills } from './backfills';
import { HappyViewClient } from './client';
import { requestIndex } from './request-index';

const AUDIENCE = 'did:web:localnetwork.computer';

const DJ = 'did:plc:dj';

const LOCALLIBRARY = [COLLECTION.djSet, COLLECTION.djProfile, COLLECTION.locallibrarySettings];

const backfillInput = z.object({ collection: z.string(), did: z.string() });

describe('requestIndex', () => {
	let key: Secp256k1Keypair;
	/** Each backfill HappyView was asked for, as `collection did`. */
	let backfilled: string[];
	/** What HappyView says of each collection's backfill job, or null for a job it doesn't list. */
	let status: (collection: string) => string | null;

	/** A HappyView that keeps no records, only answers backfills. Its job ids are the collections. */
	const happyView = ({ admin = true } = {}) =>
		new HappyViewClient({
			url: 'http://happyview.test',
			apiKey: admin ? 'hv_test' : undefined,
			fetch: async (input, init) => {
				const request = new Request(input, init);
				const { pathname } = new URL(request.url);

				if (pathname === '/admin/backfill') {
					const { collection, did } = backfillInput.parse(await request.json());
					backfilled.push(`${collection} ${did}`);

					return Response.json({ id: collection, status: 'running' });
				}

				if (pathname === '/admin/backfill/status')
					return Response.json(
						LOCALLIBRARY.flatMap((id) => {
							const job = status(id);

							return job ? [{ id, status: job }] : [];
						})
					);

				return new Response('not found', { status: 404 });
			}
		});

	const token = (claims: JsonObject = {}, signer = key) =>
		serviceJwt(signer, {
			iss: DJ,
			aud: AUDIENCE,
			lxm: REQUEST_INDEX,
			exp: Math.floor(Date.now() / 1000) + 60,
			...claims
		});

	const call = async (backfills: Backfills, authorization: string | null) => {
		const response = await requestIndex(authorization, {
			audience: AUDIENCE,
			getSigningKey: async () => key.did(),
			backfills,
			log: silentLogger
		});

		return { status: response.status, body: z.json().parse(await response.json()) };
	};

	beforeEach(async () => {
		key = await Secp256k1Keypair.create();
		backfilled = [];
		status = () => 'completed';
	});
	afterEach(() => {
		vi.useRealTimers();
	});

	it("has HappyView re-read the caller's locallibrary records, and says it finished", async () => {
		const answer = await call(new Backfills(happyView(), silentLogger), await token());

		expect(answer).toEqual({ status: 200, body: { indexed: true } });
		expect(() => lexicons.assertValidXrpcOutput(REQUEST_INDEX, answer.body)).not.toThrow();
		expect(backfilled.toSorted()).toEqual(LOCALLIBRARY.map((collection) => `${collection} ${DJ}`).toSorted());
	});

	it.each<[string, () => Promise<string | null>]>([
		['no token', async () => null],
		['an expired token', () => token({ exp: Math.floor(Date.now() / 1000) - 10 })],
		["another service's token", () => token({ aud: 'did:web:elsewhere.test' })],
		["another method's token", () => token({ lxm: 'com.atproto.space.notifyWrite' })],
		['a token another key signed', async () => token({}, await Secp256k1Keypair.create())]
	])('turns away %s, and re-reads nothing', async (_label, authorization) => {
		const answer = await call(new Backfills(happyView(), silentLogger), await authorization());

		expect(answer).toEqual({
			status: 401,
			body: { error: 'AuthenticationRequired', message: 'Invalid service auth' }
		});
		expect(backfilled).toEqual([]);
	});

	it('answers a call within ten minutes from the last re-read, and re-reads again after', async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		const backfills = new Backfills(happyView(), silentLogger);

		await call(backfills, await token());
		vi.advanceTimersByTime(9 * 60_000);
		expect(await call(backfills, await token())).toEqual({ status: 200, body: { indexed: true } });
		expect(backfilled).toHaveLength(3);

		vi.advanceTimersByTime(60_000);
		await call(backfills, await token());
		expect(backfilled).toHaveLength(6);
	});

	it('says a failed re-read is not done, and tries it again on the next call', async () => {
		const backfills = new Backfills(happyView(), silentLogger);
		status = (collection) => (collection === COLLECTION.djProfile ? 'failed' : 'completed');

		expect(await call(backfills, await token())).toEqual({ status: 200, body: { indexed: false } });
		status = () => 'completed';
		expect(await call(backfills, await token())).toEqual({ status: 200, body: { indexed: true } });
		expect(backfilled.filter((entry) => entry === `${COLLECTION.djProfile} ${DJ}`)).toHaveLength(2);
		expect(backfilled.filter((entry) => entry === `${COLLECTION.djSet} ${DJ}`)).toHaveLength(1);
	});

	it("says a re-read HappyView hasn't finished isn't done, and leaves it running instead of starting another", async () => {
		const backfills = new Backfills(happyView(), silentLogger);
		status = (collection) => (collection === COLLECTION.locallibrarySettings ? null : 'completed');

		expect(await call(backfills, await token())).toEqual({ status: 200, body: { indexed: false } });
		expect(await call(backfills, await token())).toEqual({ status: 200, body: { indexed: false } });
		expect(backfilled).toHaveLength(3);
	});

	it("says nothing was re-read without HappyView's API key", async () => {
		const answer = await call(new Backfills(happyView({ admin: false }), silentLogger), await token());

		expect(answer).toEqual({ status: 200, body: { indexed: false } });
		expect(backfilled).toEqual([]);
	});
});
