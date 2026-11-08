import type { JsonObject } from '@atproto/lex-json';
import { Secp256k1Keypair } from '@atproto/crypto';
import { beforeAll, describe, expect, it } from 'vitest';
import { GET_FLAGS, REQUEST_INDEX } from '#lib/nsid.ts';
import { silentLogger } from '../logger';
import { RateLimiter } from '../rate-limit';
import { serviceJwt } from '../testing/service-jwt';
import { getFlags } from './get-flags';

const DJ = 'did:plc:dj';

const AUDIENCE = 'did:web:localnetwork.test';

let keypair: Secp256k1Keypair;

beforeAll(async () => {
	keypair = await Secp256k1Keypair.create({ exportable: true });
});

const flags = {
	answers: async (did: string | null) => [
		{ key: 'parityCanary' as const, everyone: false, percentage: 25, targeted: did === DJ }
	]
};

const ask = (authorization: string | null, limit = new RateLimiter(5, 1)) =>
	getFlags(authorization, {
		audience: AUDIENCE,
		getSigningKey: async () => keypair.did(),
		flags,
		limit,
		clientIp: '203.0.113.9',
		log: silentLogger
	});

const token = (payload: JsonObject = {}) =>
	serviceJwt(keypair, {
		iss: DJ,
		aud: AUDIENCE,
		exp: Math.floor(Date.now() / 1000) + 60,
		lxm: GET_FLAGS,
		...payload
	});

describe('getFlags', () => {
	it('answers anyone without a token, with nothing targeted', async () => {
		const response = await ask(null);

		expect(response.status).toBe(200);
		expect(await response.json()).toEqual({
			flags: [{ key: 'parityCanary', everyone: false, percentage: 25, targeted: false }]
		});
		expect(response.headers.get('cache-control')).toBe('public, max-age=60');
		expect(response.headers.get('vary')).toBe('authorization');
	});

	it('answers for the token issuer, for them alone', async () => {
		const response = await ask(await token());

		expect(await response.json()).toEqual({
			flags: [{ key: 'parityCanary', everyone: false, percentage: 25, targeted: true }]
		});
		expect(response.headers.get('cache-control')).toBe('private, no-store');
		expect(response.headers.get('vary')).toBe('authorization');
	});

	it.each([
		['for another service', { aud: 'did:web:elsewhere.test' }],
		['for another method', { lxm: REQUEST_INDEX }],
		['past its expiry', { exp: 1000 }]
	])('refuses a token %s rather than answering as signed out', async (_, payload) => {
		const response = await ask(await token(payload));

		expect(response.status).toBe(401);
		expect(await response.json()).toMatchObject({ error: 'AuthenticationRequired' });
	});

	it('refuses a token signed by someone else', async () => {
		const other = await Secp256k1Keypair.create({ exportable: true });

		const forged = await serviceJwt(other, {
			iss: DJ,
			aud: AUDIENCE,
			exp: Math.floor(Date.now() / 1000) + 60,
			lxm: GET_FLAGS
		});

		expect((await ask(forged)).status).toBe(401);
	});

	it('limits tokens by address, and never the public answer', async () => {
		const limit = new RateLimiter(1, 1 / 3600);
		const authorization = await token();

		expect((await ask(authorization, limit)).status).toBe(200);
		const refused = await ask(authorization, limit);

		expect(refused.status).toBe(429);
		expect(await refused.json()).toMatchObject({ error: 'RateLimitExceeded' });
		expect((await ask(null, limit)).status).toBe(200);
	});
});
