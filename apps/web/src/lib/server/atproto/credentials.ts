import { P256Keypair } from '@atproto/crypto';
import { type JsonValue, lexParse } from '@atproto/lex-json';
import { isDidString } from '@atproto/oauth-scopes';
import { createSpaceSigHeaders, parseSpaceToken } from '@atproto/space';
import { z } from 'zod';
import type { Logger } from '../logger';
import type { IdentityResolver } from './identity';
import { parseSpaceUri } from './spaces';

/** An XRPC error response from a PDS or space host. */
export class XrpcResponseError extends Error {
	constructor(
		readonly status: number,
		readonly error: string,
		message?: string
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

		return createSpaceSigHeaders(this.key, { authorization: `Atproto-Space ${this.token}`, audience });
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

		const host = (await this.deps.identity.resolve(ref.authority)).spaceHost;
		await this.deps.identity.assertSafeEndpoint(host);
		const key = await P256Keypair.create();
		const headers = await createSpaceSigHeaders(key, { authorization: `Bearer ${delegationToken}` });

		const response = await (this.deps.fetch ?? fetch)(new URL('/xrpc/com.atproto.space.getSpaceCredential', host), {
			method: 'POST',
			redirect: 'error',
			headers: { ...headers, 'content-type': 'application/json', accept: 'application/json' },
			body: JSON.stringify({ space }),
			signal: AbortSignal.timeout(15_000)
		});

		const body = await readBody(response);

		if (!response.ok) throw toXrpcError(response.status, body);
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
	async query<T>(input: {
		space: string;
		service: string;
		audience: string;
		nsid: string;
		params: Record<string, string | number | boolean | undefined>;
		schema: z.ZodType<T>;
	}): Promise<T> {
		const response = await this.request(input);
		const text = await response.text();

		return input.schema.parse(lexParse(text));
	}

	/** Calls a credential-authenticated space procedure (e.g. registerNotify) with a JSON body and parses the response with `schema`. */
	async procedure<T>(
		input: { space: string; service: string; audience: string; nsid: string; body: unknown; schema: z.ZodType<T> },
		retry = true
	): Promise<T> {
		await this.deps.identity.assertSafeEndpoint(input.service);
		const credential = await this.get(input.space);

		const response = await (this.deps.fetch ?? fetch)(new URL(`/xrpc/${input.nsid}`, input.service), {
			method: 'POST',
			redirect: 'error',
			headers: { ...(await credential.headers(input.audience)), 'content-type': 'application/json' },
			body: JSON.stringify(input.body),
			signal: AbortSignal.timeout(15_000)
		});

		const body = await readBody(response);

		if (response.ok) return input.schema.parse(body ?? {});
		const error = toXrpcError(response.status, body);

		if (retry && isCredentialRejected(error)) {
			this.invalidate(input.space);

			return this.procedure(input, false);
		}

		throw error;
	}

	/** Like {@link query} but returns the raw response (for CAR and blob downloads). */
	async request(
		input: {
			space: string;
			service: string;
			audience: string;
			nsid: string;
			params: Record<string, string | number | boolean | undefined>;
		},
		retry = true
	): Promise<Response> {
		await this.deps.identity.assertSafeEndpoint(input.service);
		const credential = await this.get(input.space);
		const url = new URL(`/xrpc/${input.nsid}`, input.service);

		for (const [name, value] of Object.entries(input.params)) {
			if (value !== undefined) url.searchParams.set(name, String(value));
		}

		const response = await (this.deps.fetch ?? fetch)(url, {
			redirect: 'error',
			headers: await credential.headers(input.audience),
			signal: AbortSignal.timeout(30_000)
		});

		if (response.ok) return response;
		const error = toXrpcError(response.status, await readBody(response));

		if (retry && isCredentialRejected(error)) {
			this.invalidate(input.space);

			return this.request(input, false);
		}

		throw error;
	}
}

const spaceCredentialOutput = z.object({ credential: z.string() });

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

function toXrpcError(status: number, body: JsonValue | undefined): XrpcResponseError {
	const { error, message } = xrpcErrorBody.parse(body);

	return new XrpcResponseError(status, error ?? (status >= 500 ? 'UpstreamFailure' : 'InvalidRequest'), message);
}
