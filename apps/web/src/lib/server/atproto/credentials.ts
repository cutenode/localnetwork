import { P256Keypair } from '@atproto/crypto';
import { isLexMap } from '@atproto/lex-data';
import { type JsonValue, jsonToLex, lexParse } from '@atproto/lex-json';
import { isDidString } from '@atproto/oauth-scopes';
import { isSpaceAtUriString } from '@atproto/syntax';
import {
	createSpaceSigHeaders,
	encapsulate,
	formatMacRequest,
	type MacProfile,
	parseSpaceToken,
	RECORD_MAC_PROFILE,
	SpaceMacError,
	SpaceRepoVerificationError,
	type VerifyMacParams,
	verifyRecord
} from '@atproto/space';
import { z } from 'zod';
import type { Logger } from '../logger';
import { type IdentityResolver, type ResolvedIdentity, requireKemKey } from './identity';
import { isTooDeep, RecordValidationError, TOO_DEEP } from './lexicons';
import type { StoredRecord } from './repo-client';
import { parseSpaceUri, spaceRecordUri } from './spaces';

/** An XRPC error response from a PDS or space host. */
export class XrpcResponseError extends Error {
	constructor(
		readonly status: number,
		readonly error: string,
		message?: string,
		/** The method that answered with it, which says who sent it: a space's authority or a writer's repo host. */
		readonly nsid?: string
	) {
		super(message ? `${error}: ${message}` : error);
		this.name = 'XrpcResponseError';
	}
}

const CREDENTIAL_REJECTED = new Set([
	'CredentialRevoked',
	'JwtExpired',
	'BadJwtSignature',
	'InvalidToken',
	'BadSpaceSignature'
]);

export function isCredentialRejected(cause: unknown): boolean {
	return cause instanceof XrpcResponseError && (cause.status === 401 || CREDENTIAL_REJECTED.has(cause.error));
}

/**
 * Whether a response's MAC didn't verify. Not a RepoVerificationError: that also covers an archive that doesn't
 * parse, which a fresh KEM key can't fix.
 */
export function isMacFailure(cause: unknown): boolean {
	return cause instanceof SpaceMacError;
}

type RequestInput = {
	space: string;
	service: string;
	audience: string;
	nsid: string;
	params: Record<string, string | number | boolean | undefined>;
};

/** Asks the repo host for a MAC of `profile`, keyed to the author's `#atproto_kem` key. */
type MacRequest = { profile: MacProfile; kemKey: string };

/** Where a space host sends a syncer its space's updates, and the token that names the registration. */
export type NotifyRegistration = { endpoint: string; token: string };

/**
 * A space credential bound (RFC 9421 / RFC 7800 `cnf.kid`) to a fresh P-256
 * key held only in memory for the credential's lifetime.
 */
export class SpaceCredential {
	readonly expiresAt: number;
	private rejected = false;

	constructor(
		readonly space: string,
		readonly token: string,
		private readonly key: P256Keypair,
		/** Whether minting it registered for the space's updates, which last as long as it does. */
		readonly notifying = false
	) {
		this.expiresAt = parseSpaceToken('credential', token).payload.exp * 1000;
	}

	get usable(): boolean {
		return !this.rejected && Date.now() + 30_000 < this.expiresAt;
	}

	markRejected(): void {
		this.rejected = true;
	}

	/** Headers for a request whose `Atproto-Space-Audience` is the repo owner (or the authority for host methods). */
	async headers(audience: string): Promise<Record<string, string>> {
		if (!isDidString(audience)) throw new Error(`Space audience is not a DID: ${audience}`);
		const authorization = `Atproto-Space-Credential ${this.token}`;

		return (await createSpaceSigHeaders({ authorization, audience, keypair: this.key })).headers;
	}
}

export type CredentialSource = {
	/** The DID whose session mints delegation tokens (the root account). */
	getDelegationToken(space: string): Promise<string>;
};

export class SpaceCredentialManager {
	private readonly cache = new Map<string, SpaceCredential>();
	private readonly inflight = new Map<string, Promise<SpaceCredential>>();

	constructor(
		private readonly deps: {
			source: () => Promise<CredentialSource>;
			identity: IdentityResolver;
			logger: Logger;
			/** The registration a credential for `space` asks for, if the space's updates should be sent to this app. */
			notify?: (space: string) => Promise<NotifyRegistration | null>;
		}
	) {}

	async get(space: string): Promise<SpaceCredential> {
		const cached = this.cache.get(space);

		if (cached?.usable) return cached;
		const pending = this.inflight.get(space);

		if (pending) return pending;
		const minting = this.mint(space).finally(() => this.inflight.delete(space));
		this.inflight.set(space, minting);
		const credential = await minting;
		this.cache.set(space, credential);

		return credential;
	}

	/** When the space's registration for updates lapses, if the credential held for it made one. */
	registeredUntil(space: string): string | null {
		const credential = this.cache.get(space);

		return credential?.notifying ? new Date(credential.expiresAt).toISOString() : null;
	}

	invalidate(space: string): void {
		this.cache.get(space)?.markRejected();
		this.cache.delete(space);
	}

	private async mint(space: string): Promise<SpaceCredential> {
		const ref = parseSpaceUri(space);

		if (!ref) throw new Error(`Not a space URI: ${space}`);
		// An authority's PDS is its space host.
		const host = (await this.deps.identity.resolve(ref.authority)).pds;
		const notify = (await this.deps.notify?.(space)) ?? undefined;

		try {
			return await this.authorize({ space, host, notify });
		} catch (error) {
			// The host calls the endpoint back before it registers it. One it can't reach, or that doesn't know the token,
			// shouldn't cost the credential, which polling still needs.
			if (!notify || !(error instanceof XrpcResponseError && error.error === 'RegistrationNotConfirmed')) throw error;
			this.deps.logger.warn('space host could not confirm the notify endpoint; polling instead', {
				err: error,
				space,
				host
			});

			return this.authorize({ space, host });
		}
	}

	/** Trades a fresh delegation token for a credential, registering `notify` for the space's updates if it's given. */
	private async authorize(input: {
		space: string;
		host: string;
		notify?: NotifyRegistration;
	}): Promise<SpaceCredential> {
		const { space, host, notify } = input;
		// The host takes each delegation token once.
		const delegationToken = await this.delegationToken(space);
		// The credential is bound to this key (its `cnf.kid`), which signs every request it authorizes.
		const key = await P256Keypair.create();

		const nsid = 'com.atproto.space.authorizeSpaceSync';

		const response = await this.deps.identity.serviceApiFetch(new URL(`/xrpc/${nsid}`, host), {
			method: 'POST',
			redirect: 'error',
			headers: { 'content-type': 'application/json', accept: 'application/json' },
			body: JSON.stringify({ space, credentialKey: key.did(), delegationToken, notify }),
			signal: AbortSignal.timeout(15_000)
		});

		const body = await readBody(response);

		if (!response.ok) {
			const error = toXrpcError(response.status, body, nsid);

			// The authority refused the token the root's PDS minted (its audience, subject, or lifetime). Unlike
			// SpaceDeleted, which the syncer drops the space for, this says nothing about the space itself.
			if (error.error === 'InvalidDelegationToken') {
				this.deps.logger.warn('space authority rejected the delegation token', { err: error, space, host });
			}

			throw error;
		}

		const output = spaceCredentialOutput.safeParse(body);

		if (!output.success) throw new XrpcResponseError(502, 'InvalidResponse', 'No credential returned', nsid);
		const notifying = notify !== undefined;
		const credential = readAnswer(nsid, () => new SpaceCredential(space, output.data.credential, key, notifying));
		this.deps.logger.debug('minted space credential', { space, notifying });

		return credential;
	}

	private async delegationToken(space: string): Promise<string> {
		const source = await this.deps.source();

		try {
			return await source.getDelegationToken(space);
		} catch (error) {
			if (error instanceof Error && /bad token scope/i.test(error.message)) {
				throw new Error(
					'The root account session cannot mint space delegation tokens. ROOT_PASSWORD looks like an app password; com.atproto.space.getDelegationToken requires a full-access session (use the account password of a dedicated root account).',
					{ cause: error }
				);
			}

			throw error;
		}
	}

	/**
	 * Calls a credential-authenticated space query (listWriters, ...), decodes
	 * the response as lex JSON (bytes and CIDs decoded), and parses it with
	 * the method's output `schema`. Retries once with a fresh credential if it
	 * was rejected. Responses that carry records are read with {@link macRequest}
	 * instead, which leaves their depth to the caller to check.
	 */
	async query<T>(input: RequestInput & { schema: z.ZodType<T> }): Promise<T> {
		const response = await this.request(input);
		const text = await response.text();

		return readAnswer(input.nsid, () => input.schema.parse(lexParse(text)));
	}

	/** Like {@link query} but returns the raw response (for blob downloads). */
	async request(input: RequestInput): Promise<Response> {
		return this.retryRejected(input.space, () => this.send(input, {}));
	}

	/**
	 * Like {@link request}, for the methods that answer a space credential with a MAC (getRecord, getRepo,
	 * listRepoOps, getLatestCommit): asks for one keyed to a secret shared with the author's repo host,
	 * and returns what checking it needs.
	 */
	async macRequest(input: RequestInput & { mac: MacRequest }): Promise<{ response: Response; mac: VerifyMacParams }> {
		return this.retryRejected(input.space, async () => {
			if (!isDidString(input.mac.kemKey)) throw new Error(`KEM key is not a did:key: ${input.mac.kemKey}`);
			// A fresh ephemeral key for every request, retries included, so no two responses share a secret.
			const { keyShare, sharedSecret } = await encapsulate(input.mac.kemKey);
			const response = await this.send(input, { 'accept-signature': formatMacRequest(input.mac.profile, keyShare) });

			return { response, mac: { keyShare, sharedSecret, headers: response.headers } };
		});
	}

	/**
	 * Runs a MAC-checked read of `did`'s space repos with its identity and KEM key. A MAC that doesn't verify
	 * may mean the author's KEM key rotated since its DID document was cached, so it refreshes the document
	 * and reads once more; a second failure stands.
	 */
	async readVerified<T>(did: string, read: (identity: ResolvedIdentity, kemKey: string) => Promise<T>): Promise<T> {
		const attempt = async (forceRefresh: boolean) => {
			const identity = await this.deps.identity.resolve(did, forceRefresh);

			return read(identity, requireKemKey(identity));
		};

		try {
			return await attempt(false);
		} catch (error) {
			if (!isMacFailure(error)) throw error;
			this.deps.logger.info('MAC did not verify; retrying with a refreshed KEM key', { err: error, did });

			return attempt(true);
		}
	}

	/** Reads a record from a space the root account can read, checked against its MAC; null if there's none. */
	async readRecord(input: {
		space: string;
		repo: string;
		collection: string;
		rkey: string;
	}): Promise<StoredRecord | null> {
		const uri = spaceRecordUri(input.space, input.repo, input.collection, input.rkey);
		const nsid = 'com.atproto.space.getRecord';

		if (!isSpaceAtUriString(uri)) throw new Error(`Not a space record URI: ${uri}`);

		try {
			return await this.readVerified(input.repo, async ({ pds }, kemKey) => {
				const { response, mac } = await this.macRequest({
					space: input.space,
					service: pds,
					audience: input.repo,
					nsid,
					params: input,
					mac: { profile: RECORD_MAC_PROFILE, kemKey }
				});

				const text = await response.text();
				const body: JsonValue = readAnswer(nsid, () => JSON.parse(text));

				// z.json() and jsonToLex recurse, so the record, one level into the body, is checked with a stack first.
				if (isTooDeep(body, 0)) throw new RecordValidationError(input.collection, TOO_DEEP);
				const record = readAnswer(nsid, () => spaceRecordOutput.parse(body));
				// verifyRecord re-encodes the value to check its CID, so it needs the CIDs and bytes decoded.
				const value = jsonToLex(record.value);

				if (!isLexMap(value)) throw new SpaceMacError(`${uri} does not match its MAC`);

				try {
					await verifyRecord({ ...record, value }, { uri, ...mac });
				} catch (error) {
					// A MAC that doesn't verify stays a SpaceMacError, so the KEM key is refreshed. Another record than the
					// one asked for is an answer the host shouldn't give.
					if (!(error instanceof SpaceRepoVerificationError)) throw error;
					throw new XrpcResponseError(502, 'InvalidResponse', error.message, nsid);
				}

				return record;
			});
		} catch (error) {
			// A record that isn't there comes back as 400 RecordNotFound.
			if (error instanceof XrpcResponseError && error.error === 'RecordNotFound') return null;
			throw error;
		}
	}

	/** Runs `call` once more with a fresh credential if the space host rejected the one it used. */
	private async retryRejected<T>(space: string, call: () => Promise<T>): Promise<T> {
		try {
			return await call();
		} catch (error) {
			if (!isCredentialRejected(error)) throw error;
			this.invalidate(space);

			return call();
		}
	}

	private async send(input: RequestInput, headers: Record<string, string>): Promise<Response> {
		const credential = await this.get(input.space);
		const url = new URL(`/xrpc/${input.nsid}`, input.service);

		for (const [name, value] of Object.entries(input.params)) {
			if (value !== undefined) url.searchParams.set(name, String(value));
		}

		// A repo export is read as a stream. Everything else here is read whole.
		const { identity } = this.deps;
		const guarded = input.nsid === 'com.atproto.space.getRepo' ? identity.serviceFetch : identity.serviceApiFetch;

		const response = await guarded(url, {
			redirect: 'error',
			headers: { ...(await credential.headers(input.audience)), ...headers },
			signal: AbortSignal.timeout(30_000)
		});

		if (!response.ok) throw toXrpcError(response.status, await readBody(response), input.nsid);

		return response;
	}
}

const spaceCredentialOutput = z.object({ credential: z.string() });

/** What com.atproto.space.getRecord answers, kept as JSON for validateRecord. */
const spaceRecordOutput = z.object({ uri: z.string(), cid: z.string(), value: z.record(z.string(), z.json()) });

/** An XRPC error body. Fields of the wrong type are dropped, and a body that isn't an object reads as empty. */
const xrpcErrorBody = z
	.object({ error: z.string().optional().catch(undefined), message: z.string().optional().catch(undefined) })
	.catch({});

/**
 * What `read` makes of a host's answer to `nsid`. An answer it throws on (not JSON, nested deeper than lex JSON's
 * recursion goes, a key lex JSON refuses, the wrong shape, a credential that isn't a space token) is the host's
 * failure, so it rejects as one, naming the method.
 */
export function readAnswer<T>(nsid: string, read: () => T): T {
	try {
		return read();
	} catch (error) {
		throw new XrpcResponseError(502, 'InvalidResponse', error instanceof Error ? error.message : String(error), nsid);
	}
}

async function readBody(response: Response): Promise<JsonValue | undefined> {
	try {
		return await response.json();
	} catch {
		return undefined;
	}
}

function toXrpcError(status: number, body: JsonValue | undefined, nsid: string): XrpcResponseError {
	const { error, message } = xrpcErrorBody.parse(body);

	return new XrpcResponseError(status, error ?? (status >= 500 ? 'UpstreamFailure' : 'InvalidRequest'), message, nsid);
}
