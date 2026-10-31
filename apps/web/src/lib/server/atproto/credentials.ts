import { P256Keypair } from '@atproto/crypto';
import { isLexMap } from '@atproto/lex-data';
import { type JsonValue, jsonToLex, lexParse } from '@atproto/lex-json';
import { isDidString } from '@atproto/oauth-scopes';
import {
	createSpaceSigHeaders,
	encapsulate,
	formatWantMacHeader,
	MAC_HEADER,
	type MacType,
	parseMacHeader,
	parseSpaceToken,
	RECORD_MAC_TYPE,
	RepoVerificationError,
	SpaceMacError,
	verifyRecord,
	WANT_MAC_HEADER
} from '@atproto/space';
import { z } from 'zod';
import type { Logger } from '../logger';
import { type IdentityResolver, type ResolvedIdentity, requireKemKey } from './identity';
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

/** Whether a response's MAC didn't verify. `verifyRepoStar` reports a commit that fails its MAC as a RepoVerificationError. */
export function isMacFailure(cause: unknown): boolean {
	return cause instanceof SpaceMacError || cause instanceof RepoVerificationError;
}

type RequestInput = {
	space: string;
	service: string;
	audience: string;
	nsid: string;
	params: Record<string, string | number | boolean | undefined>;
};

/** Asks the repo host for a MAC of `type`, keyed to the author's `#atproto_kem` key. */
type MacRequest = { type: MacType; kemKey: string };

/**
 * What a response's MAC is checked with: this request's shared secret and the MAC the response carried.
 * Throws a SpaceMacError if it carried none, as a listRepoOps page without a commit doesn't.
 */
type MacParams = () => { sharedSecret: Uint8Array; mac: Uint8Array };

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
		private readonly key: P256Keypair
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

		return createSpaceSigHeaders(this.key, { authorization: `Atproto-Space-Credential ${this.token}`, audience });
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
			fetch?: typeof fetch;
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

	invalidate(space: string): void {
		this.cache.get(space)?.markRejected();
		this.cache.delete(space);
	}

	private async mint(space: string): Promise<SpaceCredential> {
		const ref = parseSpaceUri(space);

		if (!ref) throw new Error(`Not a space URI: ${space}`);
		const source = await this.deps.source();
		let delegationToken: string;

		try {
			delegationToken = await source.getDelegationToken(space);
		} catch (error) {
			if (error instanceof Error && /bad token scope/i.test(error.message)) {
				throw new Error(
					'The root account session cannot mint space delegation tokens. ROOT_PASSWORD looks like an app password; com.atproto.space.getDelegationToken requires a full-access session (use the account password of a dedicated root account).',
					{ cause: error }
				);
			}

			throw error;
		}

		// An authority's PDS is its space host.
		const host = await this.deps.identity.safePds(ref.authority);
		// The credential is bound to this key (its `cnf.kid`), which signs every request it authorizes.
		const key = await P256Keypair.create();

		const response = await (this.deps.fetch ?? fetch)(new URL('/xrpc/com.atproto.space.getSpaceCredential', host), {
			method: 'POST',
			redirect: 'error',
			headers: { 'content-type': 'application/json', accept: 'application/json' },
			body: JSON.stringify({ space, credentialKid: key.did(), delegationToken }),
			signal: AbortSignal.timeout(15_000)
		});

		const body = await readBody(response);

		if (!response.ok) {
			const error = toXrpcError(response.status, body, 'com.atproto.space.getSpaceCredential');

			// The authority refused the token the root's PDS minted (its audience, subject, or lifetime). Unlike
			// SpaceDeleted, which the syncer drops the space for, this says nothing about the space itself.
			if (error.error === 'InvalidDelegationToken') {
				this.deps.logger.warn({ err: error, space, host }, 'space authority rejected the delegation token');
			}

			throw error;
		}

		const output = spaceCredentialOutput.safeParse(body);

		if (!output.success) throw new XrpcResponseError(502, 'InvalidResponse', 'No credential returned');
		this.deps.logger.debug({ space }, 'minted space credential');

		return new SpaceCredential(space, output.data.credential, key);
	}

	/**
	 * Calls a credential-authenticated space query (listRepos, listRepoOps,
	 * getRecord, ...), decodes the response as lex JSON (bytes and CIDs
	 * decoded), and parses it with the method's output `schema`. Retries once
	 * with a fresh credential if it was rejected.
	 */
	async query<T>(input: RequestInput & { schema: z.ZodType<T> }): Promise<T> {
		const response = await this.request(input);
		const text = await response.text();

		return input.schema.parse(lexParse(text));
	}

	/** Like {@link query}, for a method that answers with a MAC (see {@link macRequest}). */
	async macQuery<T>(
		input: RequestInput & { schema: z.ZodType<T>; mac: MacRequest }
	): Promise<{ body: T; macParams: MacParams }> {
		const { response, macParams } = await this.macRequest(input);
		const text = await response.text();

		return { body: input.schema.parse(lexParse(text)), macParams };
	}

	/** Calls a credential-authenticated space procedure (e.g. registerNotify) with a JSON body and parses the response with `schema`. */
	async procedure<T>(input: {
		space: string;
		service: string;
		audience: string;
		nsid: string;
		body: unknown;
		schema: z.ZodType<T>;
	}): Promise<T> {
		await this.deps.identity.assertSafeEndpoint(input.service);

		return this.retryRejected(input.space, async () => {
			const credential = await this.get(input.space);

			const response = await (this.deps.fetch ?? fetch)(new URL(`/xrpc/${input.nsid}`, input.service), {
				method: 'POST',
				redirect: 'error',
				headers: { ...(await credential.headers(input.audience)), 'content-type': 'application/json' },
				body: JSON.stringify(input.body),
				signal: AbortSignal.timeout(15_000)
			});

			const body = await readBody(response);

			if (!response.ok) throw toXrpcError(response.status, body, input.nsid);

			return input.schema.parse(body ?? {});
		});
	}

	/** Like {@link query} but returns the raw response (for blob downloads). */
	async request(input: RequestInput): Promise<Response> {
		await this.deps.identity.assertSafeEndpoint(input.service);

		return this.retryRejected(input.space, () => this.send(input, {}));
	}

	/**
	 * Like {@link request}, for the methods that answer a space credential with a MAC (getRecord, getRepo,
	 * listRepoOps, getLatestCommit): asks for one keyed to a secret shared with the author's repo host,
	 * and returns what checking it needs.
	 */
	async macRequest(input: RequestInput & { mac: MacRequest }): Promise<{ response: Response; macParams: MacParams }> {
		await this.deps.identity.assertSafeEndpoint(input.service);

		return this.retryRejected(input.space, async () => {
			// A fresh ephemeral key for every request, retries included, so no two responses share a secret.
			const { keyShare, sharedSecret } = await encapsulate(input.mac.kemKey);
			const response = await this.send(input, { [WANT_MAC_HEADER]: formatWantMacHeader(input.mac.type, keyShare) });
			const header = response.headers.get(MAC_HEADER) ?? undefined;

			return { response, macParams: () => ({ sharedSecret, mac: parseMacHeader(header, input.mac.type) }) };
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
			this.deps.logger.info({ err: error, did }, 'MAC did not verify; retrying with a refreshed KEM key');

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

		try {
			return await this.readVerified(input.repo, async ({ pds }, kemKey) => {
				const { response, macParams } = await this.macRequest({
					space: input.space,
					service: pds,
					audience: input.repo,
					nsid: 'com.atproto.space.getRecord',
					params: input,
					mac: { type: RECORD_MAC_TYPE, kemKey }
				});

				const record = spaceRecordOutput.parse(await response.json());
				// verifyRecord re-encodes the value to check its CID, so it needs the CIDs and bytes decoded.
				const value = jsonToLex(record.value);

				if (!isLexMap(value) || !(await verifyRecord({ ...record, value }, { uri, ...macParams() }))) {
					throw new SpaceMacError(`${uri} does not match its MAC`);
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

		const response = await (this.deps.fetch ?? fetch)(url, {
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
