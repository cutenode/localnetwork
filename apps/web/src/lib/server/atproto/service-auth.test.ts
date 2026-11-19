import { Secp256k1Keypair } from '@atproto/crypto';
import type { JsonObject, JsonValue } from '@atproto/lex-json';
import * as fc from 'fast-check';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { serviceJwt } from '../testing/service-jwt';
import { ServiceAuthError, verifyServiceJwt } from './service-auth';

describe('verifyServiceJwt', () => {
	const audience = 'did:web:localnetwork.computer';
	const lxm = 'computer.localnetwork.flags.getFlags';
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
		['malformed token', 'Bearer abc', 'poorly formatted'],
		['token whose parts are not JSON', 'Bearer a.b.c', 'Malformed']
	])('rejects a %s', async (_label, header, message) => {
		await expect(verifyServiceJwt(header, { audience, lxm, getSigningKey: async () => '' })).rejects.toThrow(message);
	});

	it('rejects wrong audience, a wrong or missing method, expiry, issuer, typ, and signature', async () => {
		const key = await Secp256k1Keypair.create();
		const other = await Secp256k1Keypair.create();
		const opts = { audience, lxm, getSigningKey: async () => key.did() };

		const cases: [JsonObject, JsonObject, string][] = [
			[{ iss: 'did:plc:a', aud: 'did:web:other', lxm, exp: now + 60 }, {}, 'audience'],
			[{ iss: 'did:plc:a', aud: audience, lxm: 'com.example.other', exp: now + 60 }, {}, 'method'],
			[{ iss: 'did:plc:a', aud: audience, exp: now + 60 }, {}, 'missing jwt lexicon method'],
			[{ iss: 'did:plc:a', aud: audience, lxm, exp: now - 10 }, {}, 'expired'],
			[{ iss: 'not-a-did', aud: audience, lxm, exp: now + 60 }, {}, 'iss is not a valid did'],
			[{ iss: 'did:plc:a', aud: audience, lxm, exp: now + 60 }, { typ: 'at+jwt' }, 'Invalid jwt type']
		];

		for (const [payload, header, message] of cases) {
			await expect(verifyServiceJwt(await serviceJwt(key, payload, header), opts)).rejects.toThrow(
				new RegExp(message, 'i')
			);
		}

		const forged = await serviceJwt(other, { iss: 'did:plc:a', aud: audience, lxm, exp: now + 60 });
		await expect(verifyServiceJwt(forged, opts)).rejects.toBeInstanceOf(ServiceAuthError);

		// A signing key it can't read verifies nothing.
		const signed = await serviceJwt(key, { iss: 'did:plc:a', aud: audience, lxm, exp: now + 60 });
		await expect(verifyServiceJwt(signed, { ...opts, getSigningKey: async () => 'did:key:nope' })).rejects.toThrow(
			'could not verify jwt signature'
		);
	});

	describe('for any Authorization header', () => {
		// Fixed time, so `exp` is checked against the same clock every run.
		const NOW = 1_800_000_000;
		let key: Secp256k1Keypair;
		let other: Secp256k1Keypair;
		const opts = () => ({ audience, lxm, getSigningKey: async () => key.did() });

		beforeAll(async () => {
			[key, other] = await Promise.all([Secp256k1Keypair.create(), Secp256k1Keypair.create()]);
		});

		beforeEach(() => {
			vi.useFakeTimers({ toFake: ['Date'] });
			vi.setSystemTime(NOW * 1000);
		});

		afterEach(() => {
			vi.useRealTimers();
		});

		const b64 = (value: string) => Buffer.from(value).toString('base64url');
		const jsonPart = fc.jsonValue().map((value) => b64(JSON.stringify(value)));
		const anyPart = fc.oneof(jsonPart, fc.base64String(), fc.string());

		const unsigned = fc.oneof(
			fc.string(),
			fc.string().map((token) => `Bearer ${token}`),
			fc.tuple(anyPart, anyPart, anyPart).map((parts) => `Bearer ${parts.join('.')}`),
			// alg none, which carries no signature at all.
			fc
				.tuple(fc.constantFrom('none', 'None', 'NONE'), fc.constantFrom('', 'e30'))
				.map(
					([alg, signature]) =>
						`Bearer ${b64(JSON.stringify({ alg, typ: 'JWT' }))}.${b64(JSON.stringify({ iss: 'did:plc:a', aud: audience, lxm, exp: NOW + 60 }))}.${signature}`
				)
		);

		it('rejects every header that is not a signed token', async () => {
			await fc.assert(
				fc.asyncProperty(fc.option(unsigned, { nil: undefined }), async (header) => {
					await expect(verifyServiceJwt(header, opts())).rejects.toBeInstanceOf(ServiceAuthError);
				})
			);
		});

		const MISSING = Symbol('missing');

		type Token = {
			alg: string;
			typ: string | typeof MISSING;
			iss: string;
			aud: JsonValue | typeof MISSING;
			lxm: JsonValue | typeof MISSING;
			exp: JsonValue | typeof MISSING;
			signer: 'key' | 'other';
			/** Which character of the signed token to change, if any: in the header, the payload, or the signature. */
			tamper: null | { part: 0 | 1 | 2; at: number };
		};

		const GOOD: Token = {
			alg: 'ES256K',
			typ: 'JWT',
			iss: 'did:plc:a',
			aud: audience,
			lxm,
			exp: NOW + 60,
			signer: 'key',
			tamper: null
		};

		/** JWT types for other uses, which no service token has. */
		const NOT_SERVICE_TOKENS = new Set<Token['typ']>(['at+jwt', 'refresh+jwt', 'dpop+jwt']);

		/** What each part of a token can be instead. */
		const faults = {
			alg: fc.oneof(fc.constantFrom('ES256', 'none', 'HS256'), fc.string()),
			typ: fc.constantFrom('at+jwt', 'refresh+jwt', 'dpop+jwt', 'jwt', MISSING),
			iss: fc.oneof(fc.constantFrom('did:plc:a#atproto_pds', 'not-a-did', ''), fc.string()),
			aud: fc.oneof(fc.constantFrom('did:web:other', MISSING), fc.jsonValue()),
			lxm: fc.oneof(fc.constantFrom('com.atproto.space.notifyRepoUpdate', MISSING), fc.jsonValue()),
			exp: fc.oneof(
				fc.integer({ min: NOW - 3600, max: NOW + 3600 }),
				fc.constantFrom(MISSING, null, 'tomorrow', `${NOW + 60}`),
				fc.jsonValue()
			),
			signer: fc.constant('other'),
			tamper: fc.record({ part: fc.constantFrom(0, 1, 2), at: fc.nat() })
		} satisfies { [name in keyof Token]: fc.Arbitrary<Token[name]> };

		/** A good token with up to two of its parts changed, so each fault comes up alone often enough to be what's tested. */
		const token = fc
			.record(faults)
			.chain((bad) =>
				fc
					.subarray<keyof Token>(['alg', 'typ', 'iss', 'aud', 'lxm', 'exp', 'signer', 'tamper'], { maxLength: 2 })
					.map((changed): Token => ({ ...GOOD, ...Object.fromEntries(changed.map((name) => [name, bad[name]])) }))
			);

		/** The claims a token carries, with the ones generated as missing left out. */
		const present = (claims: Record<string, JsonValue | typeof MISSING>): JsonObject =>
			Object.fromEntries(Object.entries(claims).filter((entry): entry is [string, JsonValue] => entry[1] !== MISSING));

		/** An expiry atproto takes: a number of seconds, now or later. */
		const unexpired = z.number().min(NOW);

		/** `jwt` with one character of `part` changed to another base64url character. */
		function tampered(jwt: string, { part, at }: { part: 0 | 1 | 2; at: number }): string {
			const parts = jwt.slice('Bearer '.length).split('.');
			const text = parts[part];
			// The signature's first character, whose six bits are all used: a later one may only change padding bits.
			const index = part === 2 ? 0 : at % text.length;
			parts[part] = text.slice(0, index) + (text[index] === 'A' ? 'B' : 'A') + text.slice(index + 1);

			return `Bearer ${parts.join('.')}`;
		}

		it('verifies a token only when the issuer signed it for this audience and method and it has not expired', async () => {
			await fc.assert(
				fc.asyncProperty(token, async ({ alg, typ, iss, aud, lxm: method, exp, signer, tamper }) => {
					const signed = await serviceJwt(
						signer === 'key' ? key : other,
						present({ iss, aud, lxm: method, exp }),
						present({ alg, typ })
					);

					const header = tamper ? tampered(signed, tamper) : signed;

					const valid =
						alg === 'ES256K' &&
						!NOT_SERVICE_TOKENS.has(typ) &&
						(iss === 'did:plc:a' || iss === 'did:plc:a#atproto_pds') &&
						aud === audience &&
						method === lxm &&
						unexpired.safeParse(exp).success &&
						signer === 'key' &&
						!tamper;

					// The claims it verified, or the kind of error it threw.
					const verified = await verifyServiceJwt(header, opts()).then(
						(payload) => ({ iss: payload.iss, aud: payload.aud, lxm: payload.lxm, exp: payload.exp }),
						(error: Error) => (error instanceof ServiceAuthError ? 'ServiceAuthError' : String(error))
					);

					expect(verified).toEqual(valid ? { iss: 'did:plc:a', aud, lxm, exp } : 'ServiceAuthError');
				})
			);
		});

		// The property's counterexample at seed -567532388: @atproto/crypto skips its alg check for an empty alg.
		it('rejects a token the issuer signed with an empty alg', async () => {
			const signed = await serviceJwt(key, { iss: 'did:plc:a', aud: audience, lxm, exp: NOW + 60 }, { alg: '' });

			await expect(verifyServiceJwt(signed, opts())).rejects.toBeInstanceOf(ServiceAuthError);
		});
	});
});
