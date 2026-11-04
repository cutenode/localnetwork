import { once } from 'node:events';
import { type AddressInfo, createServer, type Socket } from 'node:net';
import { P256Keypair, Secp256k1Keypair } from '@atproto/crypto';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { silentLogger } from '../logger';
import { collectingGarbage } from '../testing/gc';
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

describe('IdentityResolver, given what a DID document and its handle say', () => {
	const did = 'did:plc:ewvi7nxzyoun6zhxrhs64oiz';
	const pds = 'https://pds.example.com';

	beforeEach(() => {
		vi.useFakeTimers({ toFake: ['Date'] });

		return () => {
			vi.useRealTimers();
			vi.unstubAllGlobals();
		};
	});

	/**
	 * A resolver whose PLC directory serves `doc` for `did`, whose handles resolve themselves to `handles` (a handle not
	 * listed doesn't), and that falls back to a handle resolver service when given one. Counts each handle lookup.
	 */
	const resolver = ({
		doc,
		handles = {},
		handleResolverUrl
	}: { doc?: DidDoc; handles?: Record<string, string>; handleResolverUrl?: string } = {}) => {
		const identity = new IdentityResolver({
			plcUrl: 'https://plc.directory',
			handleResolverUrl,
			allowPrivateNetwork: true,
			logger: silentLogger,
			fetch: async (input) => {
				const asked = decodeURIComponent(new URL(input instanceof Request ? input.url : input).pathname.slice(1));

				return asked === did && doc ? Response.json(doc) : new Response(null, { status: 404 });
			}
		});

		const lookups: string[] = [];
		vi.spyOn(identity.resolver.handle, 'resolve').mockImplementation(async (handle) => {
			lookups.push(handle);

			if (handle === 'broken.example.com') throw new Error('DNS timed out');

			// SAFETY: every DID these tests list is a did:plc.
			return (handles[handle] ?? null) as ResolvedHandle;
		});

		return { identity, lookups };
	};

	/** A DID document for `did` with `methods` as its verification methods, claiming @alice.example.com. */
	const document = (methods: { id: string; type: string; publicKeyMultibase: string }[], service = pds) => ({
		id: did,
		alsoKnownAs: ['at://alice.example.com'],
		verificationMethod: methods.map((method) => ({ ...method, id: `${did}#${method.id}`, controller: did })),
		service: service ? [{ id: '#atproto_pds', type: 'AtprotoPersonalDataServer', serviceEndpoint: service }] : []
	});

	type DidDoc = ReturnType<typeof document>;

	const multikey = async (keypair: Promise<{ did(): string }>) => (await keypair).did().slice('did:key:'.length);

	it('remembers what a handle resolved to for five minutes, however it was typed', async () => {
		const { identity, lookups } = resolver({ handles: { 'alice.example.com': did } });

		expect(await identity.resolveHandle(' @Alice.Example.com')).toBe(did);
		expect(await identity.resolveHandle('alice.example.com')).toBe(did);
		expect(lookups).toEqual(['alice.example.com']);

		vi.setSystemTime(Date.now() + 5 * 60_000);
		expect(await identity.resolveHandle('alice.example.com')).toBe(did);
		expect(lookups).toHaveLength(2);
	});

	it('asks the handle resolver service about a handle that does not resolve itself, trusting only a DID from it', async () => {
		vi.stubGlobal('fetch', async (url: URL) => {
			switch (url.searchParams.get('handle')) {
				case 'broken.example.com':
					return Response.json({ did });
				case 'odd.example.com':
					return Response.json({ did: 'alice' });
				default:
					return new Response(null, { status: 400 });
			}
		});
		const { identity } = resolver({ handleResolverUrl: 'https://resolver.test' });

		expect(await identity.resolveHandle('broken.example.com')).toBe(did);
		expect(await identity.resolveHandle('gone.example.com')).toBeNull();
		expect(await identity.resolveHandle('odd.example.com')).toBeNull();
		expect(await resolver().identity.resolveHandle('broken.example.com')).toBeNull();
	});

	it('turns a handle into its DID, and says which handle it could not resolve', async () => {
		const { identity } = resolver({ handles: { 'alice.example.com': did } });

		expect(await identity.resolveIdentifier('@alice.example.com')).toBe(did);
		await expect(identity.resolveIdentifier('@bob.example.com')).rejects.toThrow(
			new IdentityError('Could not resolve @bob.example.com')
		);
	});

	it('reads the PDS, signing key, and KEM key a DID document publishes, and the handle that resolves back', async () => {
		const signing = await multikey(Secp256k1Keypair.create());
		const kem = await multikey(P256Keypair.create());

		const { identity } = resolver({
			doc: document(
				[
					{ id: 'atproto', type: 'Multikey', publicKeyMultibase: signing },
					{ id: 'atproto_kem', type: 'Multikey', publicKeyMultibase: kem }
				],
				`${pds}/`
			),
			handles: { 'alice.example.com': did }
		});

		expect(await identity.resolve(did)).toEqual({
			did,
			handle: 'alice.example.com',
			pds,
			signingKey: `did:key:${signing}`,
			kemKey: `did:key:${kem}`
		});
		expect(await identity.signingKey(did)).toBe(`did:key:${signing}`);
	});

	it('leaves out a handle it cannot check or none is claimed, and a KEM key of a kind it cannot read', async () => {
		vi.stubGlobal('fetch', async () => {
			throw new Error('resolver down');
		});
		const signing = await multikey(Secp256k1Keypair.create());

		const { identity } = resolver({
			doc: document([
				{ id: 'atproto', type: 'Multikey', publicKeyMultibase: signing },
				{ id: 'atproto_kem', type: 'UnknownKey2030', publicKeyMultibase: signing }
			]),
			handleResolverUrl: 'https://resolver.test'
		});

		expect(await identity.resolve(did)).toMatchObject({ handle: null, kemKey: null });

		const unclaimed = {
			...document([{ id: 'atproto', type: 'Multikey', publicKeyMultibase: signing }]),
			alsoKnownAs: []
		};

		expect(await resolver({ doc: unclaimed }).identity.resolve(did)).toMatchObject({ handle: null, kemKey: null });
	});

	it('refuses a DID document without a PDS, or without a signing key it can read', async () => {
		const signing = await multikey(Secp256k1Keypair.create());
		const resolving = (doc: DidDoc) => resolver({ doc }).identity.resolve(did);

		await expect(
			resolving(document([{ id: 'atproto', type: 'Multikey', publicKeyMultibase: signing }], ''))
		).rejects.toThrow(`${did} has no PDS endpoint`);
		await expect(resolving(document([]))).rejects.toThrow(`${did} has no atproto signing key`);
		await expect(
			resolving(document([{ id: 'atproto', type: 'UnknownKey2030', publicKeyMultibase: signing }]))
		).rejects.toThrow(`${did} has an unsupported #atproto key type (UnknownKey2030)`);
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

	it("gives a service up at the caller's time limit when it never answers, while garbage is collected", async () => {
		// Connections it takes, which it holds open without a word.
		const held: Socket[] = [];
		const server = createServer((socket) => void held.push(socket));

		await once(server.listen(0, '127.0.0.1'), 'listening');
		// SAFETY: a server listening on a TCP port reports an AddressInfo.
		const { port } = server.address() as AddressInfo;

		const dev = new IdentityResolver({
			plcUrl: 'https://plc.directory',
			allowPrivateNetwork: true,
			logger: silentLogger
		});

		try {
			// The caller's limit, as SpaceCredentialManager, fetchBlob and the recording route pass theirs.
			const asked = dev.serviceFetch(`http://127.0.0.1:${port}/`, {
				redirect: 'error',
				signal: AbortSignal.timeout(200)
			});

			await expect(collectingGarbage(asked)).rejects.toMatchObject({ name: 'TimeoutError' });
			expect(held).toHaveLength(1);
		} finally {
			for (const socket of held) socket.destroy();
			await new Promise((resolve) => server.close(resolve));
		}
	});
});
