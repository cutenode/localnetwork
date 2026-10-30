import { isIP } from 'node:net';
import { lookup } from 'node:dns/promises';
import {
	type DidDocument,
	getHandle,
	getPdsEndpoint,
	getVerificationMaterial,
	isValidDidDoc
} from '@atproto/common-web';
import { getDidKeyFromMultibase } from '@atproto/identity';
import { isValidDid, isValidHandle, isValidTld } from '@atproto/syntax';
import { type AtprotoIdentityDidMethods, createDidResolver, type DidResolver } from '@atproto-labs/did-resolver';
import { safeFetchWrap } from '@atproto-labs/fetch-node';
import { AtprotoHandleResolverNode } from '@atproto-labs/handle-resolver-node';
import { z } from 'zod';
import type { Logger } from '../logger';

export type { DidDocument };

export type ResolvedIdentity = {
	did: string;
	/**
	 * Handle from the DID document, or null unless it resolves back to the DID. Anyone can put any
	 * handle in their alsoKnownAs, so an unverified one is never exposed.
	 */
	handle: string | null;
	pds: string;
	/** did:key used to verify the account's JWT signatures. */
	signingKey: string;
	/**
	 * did:key of the account's `#atproto_kem` key, which a MAC on its space repos' responses is keyed to, or null
	 * when its DID document publishes none (or one this app can't read).
	 */
	kemKey: string | null;
};

const resolveHandleOutput = z.object({ did: z.string().startsWith('did:') });

export class IdentityError extends Error {
	constructor(message: string) {
		super(message);
		this.name = 'IdentityError';
	}
}

export type IdentityOptions = {
	plcUrl: string;
	handleResolverUrl?: string;
	allowPrivateNetwork: boolean;
	logger: Logger;
	/** What the guarded fetch sends requests with, for tests. */
	fetch?: typeof globalThis.fetch;
};

/** The largest response the guarded fetch reads: an avatar's 1 MB, with room to spare. DID documents are far smaller. */
const RESPONSE_MAX_BYTES = 2 * 1024 * 1024;

/**
 * DID and handle resolution with caching, and an SSRF guard for any service
 * endpoint the app will call. A space authority is resolved like any account:
 * its PDS is its space host.
 *
 * DIDs and handles come from anyone, and resolving them makes the app send requests to hosts they name (a did:web's
 * host, a handle's `/.well-known/atproto-did`), so those go through `fetch`: it follows no redirects, and refuses
 * private and loopback addresses at the moment it connects, so DNS can't point a checked name somewhere else after.
 * Names that could only lead somewhere private (IP addresses, ports, `localhost` and reserved TLDs) are refused before
 * any request. Development allows private addresses, where the local PDS lives.
 */
export class IdentityResolver {
	readonly resolver: { did: DidResolver<AtprotoIdentityDidMethods>; handle: AtprotoHandleResolverNode };
	/** A fetch safe to point at an address someone else chose. Callers must pass `redirect`. */
	readonly fetch: typeof globalThis.fetch;
	private readonly handleCache = new Map<string, { did: string | null; at: number }>();

	constructor(private readonly options: IdentityOptions) {
		const base = options.fetch ?? globalThis.fetch;

		this.fetch = safeFetchWrap({
			fetch: base,
			ssrfProtection: !options.allowPrivateNetwork,
			responseMaxSize: RESPONSE_MAX_BYTES,
			timeout: 15_000
		});

		this.resolver = {
			did: createDidResolver({
				fetch: this.fetch,
				plcDirectoryUrl: options.plcUrl,
				allowHttp: options.allowPrivateNetwork
			}),
			handle: new AtprotoHandleResolverNode({ safeFetch: this.fetch })
		};
	}

	/**
	 * Whether the app may look `host` up over the network: never an IP address, a host with a port, `localhost`, or a
	 * reserved TLD (`.local`, `.internal` and the like), except in development.
	 */
	private isPublicHost(host: string): boolean {
		if (this.options.allowPrivateNetwork) return true;
		const name = host.toLowerCase();

		return !isIP(name) && !name.includes(':') && name !== 'localhost' && isValidTld(name);
	}

	async resolveHandle(handle: string): Promise<string | null> {
		const normalized = handle.trim().replace(/^@/, '').toLowerCase();

		if (!isValidHandle(normalized) || !this.isPublicHost(normalized)) return null;
		const cached = this.handleCache.get(normalized);

		if (cached && Date.now() - cached.at < 5 * 60_000) return cached.did;
		let did: string | null = null;

		try {
			did = (await this.resolver.handle.resolve(normalized)) ?? null;
		} catch (error) {
			this.options.logger.debug({ err: error, handle: normalized }, 'handle resolution failed');
		}

		if (!did && this.options.handleResolverUrl) {
			did = await this.resolveHandleViaXrpc(this.options.handleResolverUrl, normalized);
		}

		this.handleCache.set(normalized, { did, at: Date.now() });

		return did;
	}

	private async resolveHandleViaXrpc(service: string, handle: string): Promise<string | null> {
		const url = new URL('/xrpc/com.atproto.identity.resolveHandle', service);
		url.searchParams.set('handle', handle);
		const response = await fetch(url, { signal: AbortSignal.timeout(5_000) });

		if (!response.ok) return null;
		const output = resolveHandleOutput.safeParse(await response.json());

		return output.success ? output.data.did : null;
	}

	/** Accepts a handle (with or without `@`) or a DID and returns the DID. */
	async resolveIdentifier(identifier: string): Promise<string> {
		const value = identifier.trim();

		if (value.startsWith('did:')) return value;
		const did = await this.resolveHandle(value);

		if (!did) throw new IdentityError(`Could not resolve @${value.replace(/^@/, '')}`);

		return did;
	}

	async resolveDidDocument(did: string, forceRefresh = false): Promise<DidDocument> {
		if (!isValidDid(did)) throw new IdentityError(`${did} isn't a DID`);
		const web = did.startsWith('did:web:') ? decodeURIComponent(did.slice('did:web:'.length)) : null;

		if (web !== null && !this.isPublicHost(web)) throw new IdentityError(`Refusing to resolve ${did}`);

		const doc = await this.resolver.did.resolve(did, { noCache: forceRefresh }).catch((cause: unknown) => {
			this.options.logger.debug({ err: cause, did }, 'DID resolution failed');

			return null;
		});

		if (!isValidDidDoc(doc)) throw new IdentityError(`Could not resolve ${did}`);

		return doc;
	}

	async resolve(did: string, forceRefresh = false): Promise<ResolvedIdentity> {
		const doc = await this.resolveDidDocument(did, forceRefresh);
		const pds = getPdsEndpoint(doc);

		if (!pds) throw new IdentityError(`${did} has no PDS endpoint`);
		const atprotoKey = getVerificationMaterial(doc, 'atproto');

		if (!atprotoKey) throw new IdentityError(`${did} has no atproto signing key`);
		// Missing on accounts whose PDS hasn't published one yet. Only reading their space repos needs it.
		const kemKey = getVerificationMaterial(doc, 'atproto_kem');
		const claimed = getHandle(doc) ?? null;
		const handle = claimed && (await this.resolveHandle(claimed).catch(() => null)) === did ? claimed : null;

		return {
			did,
			handle,
			pds: pds.replace(/\/+$/, ''),
			signingKey: toDidKey(atprotoKey, did, 'atproto'),
			kemKey: (kemKey && getDidKeyFromMultibase(kemKey)) ?? null
		};
	}

	/**
	 * The account's PDS, for the app to call, once assertSafeEndpoint passes it. That check looks the host up apart
	 * from the request, so DNS rebinding can still get past it.
	 */
	async safePds(did: string): Promise<string> {
		const { pds } = await this.resolve(did);
		await this.assertSafeEndpoint(pds);

		return pds;
	}

	async signingKey(did: string, forceRefresh = false): Promise<string> {
		return (await this.resolve(did, forceRefresh)).signingKey;
	}

	/**
	 * Refuses to talk to loopback, link-local, or private addresses unless the
	 * deployment allows it (development). Service endpoints come from DID
	 * documents, which anyone can publish.
	 */
	async assertSafeEndpoint(endpoint: string): Promise<void> {
		if (this.options.allowPrivateNetwork) return;
		const url = new URL(endpoint);

		if (url.protocol !== 'https:') throw new IdentityError(`Refusing non-HTTPS endpoint ${url.origin}`);
		const host = url.hostname.replace(/^\[|\]$/g, '');
		const addresses = isIP(host) ? [host] : (await lookup(host, { all: true })).map((entry) => entry.address);

		for (const address of addresses) {
			if (isPrivateAddress(address)) throw new IdentityError(`Refusing private network endpoint ${url.origin}`);
		}
	}
}

/** The account's KEM key, or an error saying why its space repos can't be read without one. */
export function requireKemKey(identity: ResolvedIdentity): string {
	if (!identity.kemKey) {
		throw new IdentityError(
			`${identity.did} publishes no #atproto_kem key, so its space repos can't be synced until its PDS adds one`
		);
	}

	return identity.kemKey;
}

/** Handles both Multikey and legacy (EcdsaSecp256k1VerificationKey2019 etc.) encodings. */
function toDidKey(material: { type: string; publicKeyMultibase: string }, did: string, id: string): string {
	const didKey = getDidKeyFromMultibase(material);

	if (!didKey) throw new IdentityError(`${did} has an unsupported #${id} key type (${material.type})`);

	return didKey;
}

export function isPrivateAddress(address: string): boolean {
	if (isIP(address) === 4) {
		const [a, b] = address.split('.').map(Number);

		return (
			a === 0 ||
			a === 10 ||
			a === 127 ||
			(a === 100 && b >= 64 && b <= 127) ||
			(a === 169 && b === 254) ||
			(a === 172 && b >= 16 && b <= 31) ||
			(a === 192 && b === 168) ||
			(a === 198 && (b === 18 || b === 19)) ||
			a >= 224
		);
	}

	const lower = address.toLowerCase();

	if (lower.startsWith('::ffff:')) return isPrivateAddress(lower.slice(7));

	return (
		lower === '::' ||
		lower === '::1' ||
		lower.startsWith('fc') ||
		lower.startsWith('fd') ||
		lower.startsWith('fe8') ||
		lower.startsWith('fe9') ||
		lower.startsWith('fea') ||
		lower.startsWith('feb') ||
		lower.startsWith('ff')
	);
}
