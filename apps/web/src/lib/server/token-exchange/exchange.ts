// Local Network's token exchange: locallibrary plugins sign in to services whose token endpoint
// wants a client secret, which can't ship inside an app people download. locallibrary sends here
// what it would have sent the service, and this adds the client id and secret from the
// environment, sends it on to the service's token endpoint (only ever one in services.json), and
// answers what the service answered. Codes, verifiers, tokens and secrets are never logged.
import { z } from 'zod';
import { parseJson } from '../db';
import type { Logger } from '../logger';
import { type Service, services } from './services';

/** The most of a request, or of a service's answer, read: either is a few kilobytes. */
const MAX_BYTES = 64 * 1024;

const TIMEOUT_MS = 10_000;

const value = z.string().min(1).max(8192);

/**
 * The token request locallibrary would have sent the service, as JSON (SvelteKit refuses a form
 * posted without an Origin header, as it would a browser's from another site). Whatever else it
 * has, its client_id among them, is dropped.
 */
const grantSchema = z.discriminatedUnion('grant_type', [
	z.object({
		grant_type: z.literal('authorization_code'),
		code: value,
		redirect_uri: value,
		code_verifier: value.optional()
	}),
	z.object({ grant_type: z.literal('refresh_token'), refresh_token: value, redirect_uri: value.optional() })
]);

type Grant = z.infer<typeof grantSchema>;

/** A token endpoint's answer, success or error: a JSON object, kept whole. */
const answerSchema = z.looseObject({ access_token: z.string().optional(), token_type: z.string().optional() });

const NO_STORE = { 'cache-control': 'no-store' };

/** An OAuth error answer (RFC 6749 section 5.2), which locallibrary shows as what the service said. */
function refuse(status: number, error: string, description: string): Response {
	return Response.json({ error, error_description: description }, { status, headers: NO_STORE });
}

/** `message`'s body as text, or undefined past `MAX_BYTES`, without reading more than that. */
async function readCapped(message: Request | Response): Promise<string | undefined> {
	if (!message.body) return '';
	const chunks: Uint8Array[] = [];
	let size = 0;

	for await (const chunk of message.body) {
		size += chunk.byteLength;

		if (size > MAX_BYTES) return undefined;
		chunks.push(chunk);
	}

	return Buffer.concat(chunks).toString('utf8');
}

/** The request to `service`'s token endpoint for `grant`, the way the service takes it. */
function upstream(service: Service, grant: Grant, client: { client_id: string; client_secret: string }): Request {
	const headers = { accept: 'application/json' };

	if (service.style === 'query' && grant.grant_type === 'authorization_code') {
		const url = new URL(service.tokenUrl);
		const query = { ...client, redirect_uri: grant.redirect_uri, code: grant.code };

		for (const [key, param] of Object.entries(query)) url.searchParams.set(key, param);

		return new Request(url, { headers });
	}

	return new Request(service.tokenUrl, {
		method: 'POST',
		headers: { ...headers, 'content-type': 'application/x-www-form-urlencoded' },
		body: new URLSearchParams({ ...grant, ...client })
	});
}

export type ExchangeOptions = {
	log: Logger;
	/** Where the services' client ids and secrets are read from. */
	env?: Record<string, string | undefined>;
	fetch?: typeof fetch;
};

/** Trades a code or refresh token for tokens at the service `id` names, with Local Network's client there. */
export async function exchangeToken(id: string, request: Request, options: ExchangeOptions): Promise<Response> {
	const { log, env = process.env, fetch: send = fetch } = options;
	const service = Object.hasOwn(services, id) ? services[id] : undefined;

	if (!service) return refuse(404, 'invalid_request', `Local Network doesn't sign in to "${id.slice(0, 64)}"`);

	const body = await readCapped(request);

	if (body === undefined) return refuse(413, 'invalid_request', 'The token request is too large');
	const parsed = grantSchema.safeParse(parseJson<unknown>(body, undefined));

	if (!parsed.success) return refuse(400, 'invalid_request', "That isn't a token request locallibrary sends");
	const grant = parsed.data;
	const at = { service: id, grant: grant.grant_type };
	const client = { client_id: env[service.clientIdEnv], client_secret: env[service.clientSecretEnv] };

	if (!client.client_id || !client.client_secret) {
		log.warn({ ...at, env: [service.clientIdEnv, service.clientSecretEnv] }, 'token exchange not configured');

		return refuse(503, 'temporarily_unavailable', `Local Network isn't set up to sign in to ${service.title} yet`);
	}

	if (service.style === 'query' && grant.grant_type !== 'authorization_code')
		return refuse(400, 'unsupported_grant_type', `${service.title} doesn't refresh tokens`);

	const started = performance.now();
	const configured = { client_id: client.client_id, client_secret: client.client_secret };
	let answer: Response;

	try {
		// A token endpoint doesn't redirect, and following one would send the secret along.
		answer = await send(upstream(service, grant, configured), {
			redirect: 'error',
			signal: AbortSignal.timeout(TIMEOUT_MS)
		});
	} catch (error) {
		// Only the messages: a request's address can hold the code and secret, which an error's other fields may carry.
		const cause = error instanceof Error && error.cause instanceof Error ? `: ${error.cause.message}` : '';
		const reason = error instanceof Error ? `${error.message}${cause}` : 'unknown';
		log.warn({ ...at, reason }, 'token endpoint unreachable');

		return refuse(502, 'temporarily_unavailable', `${service.title} isn't answering`);
	}

	const tokens = answerSchema.safeParse(parseJson<unknown>(await readCapped(answer), undefined));
	log.info({ ...at, status: answer.status, ms: Math.round(performance.now() - started) }, 'token exchange');

	if (!tokens.success)
		return refuse(502, 'temporarily_unavailable', `${service.title} answered something that isn't a token response`);

	// OAuth clients need a token_type, which Mixcloud doesn't document sending.
	if (answer.ok && tokens.data.access_token !== undefined) tokens.data.token_type ??= 'bearer';

	return Response.json(tokens.data, { status: answer.status, headers: NO_STORE });
}
