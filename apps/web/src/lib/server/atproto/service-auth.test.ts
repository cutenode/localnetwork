import { Secp256k1Keypair } from '@atproto/crypto';
import type { JsonObject } from '@atproto/lex-json';
import { describe, expect, it } from 'vitest';
import { serviceJwt } from '../testing/service-jwt';
import { ServiceAuthError, verifyServiceJwt } from './service-auth';

describe('verifyServiceJwt', () => {
	const audience = 'did:web:localnetwork.computer#localnetwork_syncer';
	const lxm = 'com.atproto.space.notifyWrite';
	const now = Math.floor(Date.now() / 1000);

	it('accepts a valid token and strips the issuer fragment', async () => {
		const key = await Secp256k1Keypair.create();
		const jwt = await serviceJwt(key, { iss: 'did:plc:authority#atproto_pds', aud: audience, lxm, exp: now + 60 });
		const payload = await verifyServiceJwt(jwt, { audience, lxm, getSigningKey: async () => key.did() });
		expect(payload.iss).toBe('did:plc:authority');
	});

	it('retries with a refreshed key when the cached one is stale', async () => {
		const stale = await Secp256k1Keypair.create();
		const fresh = await Secp256k1Keypair.create();
		const jwt = await serviceJwt(fresh, { iss: 'did:plc:a', aud: audience, lxm, exp: now + 60 });
		const calls: boolean[] = [];
		await verifyServiceJwt(jwt, {
			audience,
			lxm,
			getSigningKey: async (_did, force) => {
				calls.push(force);

				return force ? fresh.did() : stale.did();
			}
		});
		expect(calls).toEqual([false, true]);
	});

	it.each([
		['missing token', undefined, 'Missing'],
		['malformed token', 'Bearer abc', 'Malformed']
	])('rejects a %s', async (_label, header, message) => {
		await expect(verifyServiceJwt(header, { audience, lxm, getSigningKey: async () => '' })).rejects.toThrow(message);
	});

	it('rejects wrong audience, method, expiry, issuer, typ, and signature', async () => {
		const key = await Secp256k1Keypair.create();
		const other = await Secp256k1Keypair.create();
		const opts = { audience, lxm, getSigningKey: async () => key.did() };

		const cases: [JsonObject, JsonObject, string][] = [
			[{ iss: 'did:plc:a', aud: 'did:web:other', lxm, exp: now + 60 }, {}, 'audience'],
			[{ iss: 'did:plc:a', aud: audience, lxm: 'com.example.other', exp: now + 60 }, {}, 'method'],
			[{ iss: 'did:plc:a', aud: audience, lxm, exp: now - 10 }, {}, 'expired'],
			[{ iss: 'not-a-did', aud: audience, lxm, exp: now + 60 }, {}, 'issuer'],
			[{ iss: 'did:plc:a', aud: audience, lxm, exp: now + 60 }, { typ: 'at+jwt' }, 'Not a service auth token']
		];

		for (const [payload, header, message] of cases) {
			await expect(verifyServiceJwt(await serviceJwt(key, payload, header), opts)).rejects.toThrow(
				new RegExp(message, 'i')
			);
		}

		const forged = await serviceJwt(other, { iss: 'did:plc:a', aud: audience, lxm, exp: now + 60 });
		await expect(verifyServiceJwt(forged, opts)).rejects.toBeInstanceOf(ServiceAuthError);
	});
});
