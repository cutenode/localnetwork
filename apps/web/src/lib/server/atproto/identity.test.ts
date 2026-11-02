import { Secp256k1Keypair } from '@atproto/crypto';
import { describe, expect, it, vi } from 'vitest';
import { silentLogger } from '../logger';
import type { DidDocument } from '@atproto-labs/did-resolver';
import type { AtprotoHandleResolverNode } from '@atproto-labs/handle-resolver-node';
import { IdentityError, IdentityResolver } from './identity';

/** What handle resolution finds: the DID, or null. */
type ResolvedHandle = Awaited<ReturnType<AtprotoHandleResolverNode['resolve']>>;

describe('IdentityResolver.resolveIdentifier', () => {
	it('resolves DIDs as identifiers without network access', async () => {
		const strict = new IdentityResolver({
			plcUrl: 'https://plc.directory',
			allowPrivateNetwork: false,
			logger: silentLogger
		});

		await expect(strict.resolveIdentifier(' did:plc:abc ')).resolves.toBe('did:plc:abc');
	});
});

describe('IdentityResolver.resolve', () => {
	const did = 'did:plc:mallory';

	async function resolveClaiming(handleOwner: ResolvedHandle) {
		const key = (await Secp256k1Keypair.create()).did().slice('did:key:'.length);

		const doc: DidDocument = {
			id: did,
			alsoKnownAs: ['at://alice.example.com'],
			verificationMethod: [{ id: `${did}#atproto`, type: 'Multikey', controller: did, publicKeyMultibase: key }],
			service: [{ id: '#atproto_pds', type: 'AtprotoPersonalDataServer', serviceEndpoint: 'https://pds.example.com' }]
		};

		const identity = new IdentityResolver({
			plcUrl: 'https://plc.directory',
			allowPrivateNetwork: false,
			logger: silentLogger
		});

		// SAFETY: the resolver's return type narrows the document's `id` to the DID asked for, which this one is.
		vi.spyOn(identity.resolver.did, 'resolve').mockResolvedValue(
			doc as Awaited<ReturnType<typeof identity.resolver.did.resolve>>
		);
		vi.spyOn(identity.resolver.handle, 'resolve').mockResolvedValue(handleOwner);

		return identity.resolve(did);
	}

	it('returns a handle that resolves back to the DID', async () => {
		expect((await resolveClaiming(did)).handle).toBe('alice.example.com');
	});

	it('drops a handle the DID claims but does not control', async () => {
		expect((await resolveClaiming('did:plc:alice')).handle).toBeNull();
		expect((await resolveClaiming(null)).handle).toBeNull();
	});
});

describe('IdentityResolver, given names someone else chose', () => {
	/** A resolver whose network requests go to `requests` instead, answering each with `answer`. */
	const resolver = (allowPrivateNetwork: boolean, answer = () => new Response(null, { status: 404 })) => {
		const requests: string[] = [];

		const identity = new IdentityResolver({
			plcUrl: 'https://plc.directory',
			allowPrivateNetwork,
			logger: silentLogger,
			fetch: async (input) => {
				requests.push(input instanceof Request ? input.url : String(input));

				return answer();
			}
		});

		return { identity, requests };
	};

	it.each([
		'did:web:127.0.0.1',
		'did:web:localhost',
		'did:web:printer.local',
		'did:web:metadata.internal',
		'did:web:example.com%3A8080',
		'did:web:[::1]'
	])('refuses %s before sending any request', async (did) => {
		const { identity, requests } = resolver(false);

		await expect(identity.resolve(did)).rejects.toThrow(IdentityError);
		expect(requests).toEqual([]);
	});

	it.each(['printer.local', 'router.internal', 'localhost', '127.0.0.1', 'shop.localhost'])(
		"treats @%s as a handle that doesn't resolve, without sending any request",
		async (handle) => {
			const { identity, requests } = resolver(false);

			expect(await identity.resolveHandle(handle)).toBeNull();
			expect(requests).toEqual([]);
		}
	);

	it("sends a did:web's lookup through the fetch it was given, so the guard sees it", async () => {
		const { identity, requests } = resolver(true);

		await expect(identity.resolve('did:web:dj.localnetwork.computer')).rejects.toThrow(IdentityError);
		expect(requests).toEqual(['https://dj.localnetwork.computer/.well-known/did.json']);
	});

	it.each(['fetch', 'serviceFetch'] as const)(
		"%s won't connect to a private or loopback address, speak plain HTTP, or follow a redirect",
		async (name) => {
			const strict = new IdentityResolver({
				plcUrl: 'https://plc.directory',
				allowPrivateNetwork: false,
				logger: silentLogger
			});

			/** Why the guarded fetch refused `url`: its error's cause, as fetch reports a refused connection. */
			const refusal = (url: string, init?: RequestInit) =>
				strict[name](url, init).then(
					() => 'fetched',
					(error: Error) => (error.cause instanceof Error ? error.cause.message : error.message)
				);

			expect(await refusal('https://127.0.0.1/', { redirect: 'error' })).toBe('Hostname is a non-unicast address');
			expect(await refusal('https://[::1]/', { redirect: 'error' })).toBe('Hostname is a non-unicast address');
			expect(await refusal('https://localhost/', { redirect: 'error' })).toBe('Hostname is not a public domain');
			expect(await refusal('http://dj.localnetwork.computer/', { redirect: 'error' })).toBe(
				'Forbidden protocol "http:"'
			);
			expect(await refusal('https://dj.localnetwork.computer/')).toBe('Request redirect must be "error" or "manual"');
		}
	);
});
