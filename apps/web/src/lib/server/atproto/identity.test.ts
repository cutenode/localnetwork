import { Secp256k1Keypair } from '@atproto/crypto';
import { describe, expect, it, vi } from 'vitest';
import { silentLogger } from '../logger';
import type { DidDocument } from '@atproto-labs/did-resolver';
import type { AtprotoHandleResolverNode } from '@atproto-labs/handle-resolver-node';
import { IdentityError, IdentityResolver, isPrivateAddress } from './identity';

/** What handle resolution finds: the DID, or null. */
type ResolvedHandle = Awaited<ReturnType<AtprotoHandleResolverNode['resolve']>>;

describe('isPrivateAddress', () => {
	it.each([
		'127.0.0.1',
		'10.1.2.3',
		'172.16.0.1',
		'192.168.1.1',
		'169.254.1.1',
		'100.64.0.1',
		'0.0.0.0',
		'::1',
		'fc00::1',
		'fe80::1',
		'::ffff:127.0.0.1'
	])('flags %s', (address) => {
		expect(isPrivateAddress(address)).toBe(true);
	});
	it.each(['8.8.8.8', '1.1.1.1', '2606:4700::1111', '172.32.0.1'])('allows %s', (address) => {
		expect(isPrivateAddress(address)).toBe(false);
	});
});

describe('IdentityResolver.assertSafeEndpoint', () => {
	const strict = new IdentityResolver({
		plcUrl: 'https://plc.directory',
		allowPrivateNetwork: false,
		logger: silentLogger
	});

	it('refuses plain HTTP and private addresses', async () => {
		await expect(strict.assertSafeEndpoint('http://example.com')).rejects.toThrow(/non-HTTPS/);
		await expect(strict.assertSafeEndpoint('https://127.0.0.1:8080')).rejects.toThrow(/private/);
		await expect(strict.assertSafeEndpoint('https://[::1]')).rejects.toThrow(/private/);
	});
	it('allows anything in development mode', async () => {
		const dev = new IdentityResolver({
			plcUrl: 'https://plc.directory',
			allowPrivateNetwork: true,
			logger: silentLogger
		});

		await expect(dev.assertSafeEndpoint('http://localhost:2583')).resolves.toBeUndefined();
	});
	it('resolves DIDs as identifiers without network access', async () => {
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

	it("won't connect to a private or loopback address, speak plain HTTP, or follow a redirect", async () => {
		const strict = new IdentityResolver({
			plcUrl: 'https://plc.directory',
			allowPrivateNetwork: false,
			logger: silentLogger
		});

		/** Why the guarded fetch refused `url`: its error's cause, as fetch reports a refused connection. */
		const refusal = (url: string, init?: RequestInit) =>
			strict.fetch(url, init).then(
				() => 'fetched',
				(error: Error) => (error.cause instanceof Error ? error.cause.message : error.message)
			);

		expect(await refusal('https://127.0.0.1/', { redirect: 'error' })).toBe('Hostname is a non-unicast address');
		expect(await refusal('https://[::1]/', { redirect: 'error' })).toBe('Hostname is a non-unicast address');
		expect(await refusal('https://localhost/', { redirect: 'error' })).toBe('Hostname is not a public domain');
		expect(await refusal('http://dj.localnetwork.computer/', { redirect: 'error' })).toBe('Forbidden protocol "http:"');
		expect(await refusal('https://dj.localnetwork.computer/')).toBe('Request redirect must be "error" or "manual"');
	});
});
