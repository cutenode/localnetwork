import { isIP } from 'node:net';
import { lookup } from 'node:dns/promises';
import { getHandle, getPdsEndpoint, getServiceEndpoint, getVerificationMaterial } from '@atproto/common-web';
import { getDidKeyFromMultibase, IdResolver, MemoryCache } from '@atproto/identity';
import { z } from 'zod';
import type { Logger } from '../logger';

export type DidDocument = NonNullable<Awaited<ReturnType<IdResolver['did']['resolve']>>>;

export type ResolvedIdentity = {
	did: string;
	/**
	 * Handle from the DID document, or null unless it resolves back to the DID. Anyone can put any
	 * handle in their alsoKnownAs, so an unverified one is never exposed.
	 */
	handle: string | null;
	pds: string;
	/** did:key used to verify the account's commit and JWT signatures. */
	signingKey: string;
	/** Where this DID answers as a space authority (falls back to the PDS). */
	spaceHost: string;
	/** did:key that signs space credentials for spaces under this DID. */
	spaceKey: string;
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
};

/**
 * DID and handle resolution with caching, space-host discovery, and an SSRF
 * guard for any service endpoint the app will call.
 */
export class IdentityResolver {
	readonly resolver: IdResolver;
	private readonly handleCache = new Map<string, { did: string | null; at: number }>();

	constructor(private readonly options: IdentityOptions) {
		this.resolver = new IdResolver({
			plcUrl: options.plcUrl,
			timeout: 5_000,
			didCache: new MemoryCache(5 * 60_000, 60 * 60_000)
		});
	}

	async resolveHandle(handle: string): Promise<string | null> {
		const normalized = handle.trim().replace(/^@/, '').toLowerCase();
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
		const doc = await this.resolver.did.resolve(did, forceRefresh);

		if (!doc) throw new IdentityError(`Could not resolve ${did}`);

		return doc;
	}

	async resolve(did: string, forceRefresh = false): Promise<ResolvedIdentity> {
		const doc = await this.resolveDidDocument(did, forceRefresh);
		const pds = getPdsEndpoint(doc);

		if (!pds) throw new IdentityError(`${did} has no PDS endpoint`);
		const atprotoKey = getVerificationMaterial(doc, 'atproto');

		if (!atprotoKey) throw new IdentityError(`${did} has no atproto signing key`);

		const spaceHostEntry = (doc.service ?? []).find(
			(service) => service.id === '#atproto_space_host' || service.id === `${did}#atproto_space_host`
		);

		let spaceHost = pds;

		if (spaceHostEntry) {
			const endpoint = getServiceEndpoint(doc, { id: '#atproto_space_host', type: 'AtprotoSpaceHost' });

			if (!endpoint) throw new IdentityError(`${did} has a malformed #atproto_space_host entry`);
			spaceHost = endpoint;
		}

		const hasSpaceKey = (doc.verificationMethod ?? []).some(
			(method) => method.id === '#atproto_space' || method.id === `${did}#atproto_space`
		);

		const spaceKeyMaterial = hasSpaceKey ? getVerificationMaterial(doc, 'atproto_space') : atprotoKey;

		if (!spaceKeyMaterial) throw new IdentityError(`${did} has a malformed #atproto_space key`);

		const claimed = getHandle(doc) ?? null;
		const handle = claimed && (await this.resolveHandle(claimed).catch(() => null)) === did ? claimed : null;

		return {
			did,
			handle,
			pds: pds.replace(/\/+$/, ''),
			signingKey: toDidKey(atprotoKey, did, 'atproto'),
			spaceHost: spaceHost.replace(/\/+$/, ''),
			spaceKey: toDidKey(spaceKeyMaterial, did, 'atproto_space')
		};
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
