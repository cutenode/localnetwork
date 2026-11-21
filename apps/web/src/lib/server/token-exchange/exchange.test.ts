import * as fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { flagsOff } from '#lib/testing/flags.ts';
import { silentLogger } from '../logger';
import { remoteObject } from '../testing/remote-bodies';
import { writtenLog } from '../testing/written-log';
import { exchangeToken } from './exchange';
import { services as registered } from './services';

const env = {
	SOUNDCLOUD_CLIENT_ID: 'sc-client',
	SOUNDCLOUD_CLIENT_SECRET: 'sc-secret',
	MIXCLOUD_CLIENT_ID: 'mc-client',
	MIXCLOUD_CLIENT_SECRET: 'mc-secret'
};

/** Everything here a log line must never hold. */
const SECRETS = ['sc-secret', 'mc-secret', 'c0de', 'v3rifier', 'rt-1', 'at-1', 'at-2', 'rt-2'];

type Sent = { method: string; url: URL; form: URLSearchParams };

/**
 * Stand-ins for the services' token endpoints, at the addresses services.json names: SoundCloud
 * takes a form with the client's secret and rotates refresh tokens, Mixcloud takes a GET with
 * its query and answers only an access token. Each answers an error for a client it doesn't know.
 */
function services() {
	const sent: Sent[] = [];

	const fetch = async (input: RequestInfo | URL): Promise<Response> => {
		const request = new Request(input);
		const url = new URL(request.url);
		const form = request.method === 'POST' ? new URLSearchParams(await request.text()) : url.searchParams;
		sent.push({ method: request.method, url, form });

		if (url.origin === 'https://secure.soundcloud.com' && url.pathname === '/oauth/token') {
			const known = form.get('client_id') === 'sc-client' && form.get('client_secret') === 'sc-secret';

			if (!known) return Response.json({ error: 'invalid_client' }, { status: 401 });

			if (form.get('grant_type') === 'refresh_token' && form.get('refresh_token') !== 'rt-1')
				return Response.json({ error: 'invalid_grant', error_description: 'spent' }, { status: 400 });
			const n = form.get('grant_type') === 'refresh_token' ? 2 : 1;

			return Response.json({
				access_token: `at-${n}`,
				token_type: 'bearer',
				expires_in: 3600,
				refresh_token: `rt-${n}`
			});
		}

		if (url.origin === 'https://www.mixcloud.com' && url.pathname === '/oauth/access_token') {
			const known = form.get('client_id') === 'mc-client' && form.get('client_secret') === 'mc-secret';

			return known
				? Response.json({ access_token: 'at-1' })
				: Response.json({ error: 'incorrect_client_credentials' }, { status: 400 });
		}

		return new Response('not found', { status: 404 });
	};

	return { sent, fetch };
}

/** A logger writing into `lines`, to check what it says and never says. */
function capture() {
	const { logger, lines } = writtenLog();

	return { lines, log: logger };
}

/** What locallibrary posts: `text`, said to be JSON. */
function posted(text: string): Request {
	return new Request('http://localnetwork.test/api/token-exchange/x', {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: text
	});
}

const post = (grant: Record<string, string>) => posted(JSON.stringify(grant));

const code = {
	grant_type: 'authorization_code',
	code: 'c0de',
	code_verifier: 'v3rifier',
	redirect_uri: 'http://127.0.0.1:4/callback'
};

describe('exchangeToken', () => {
	it("sends SoundCloud the code with Local Network's client and secret, and answers its tokens as they were", async () => {
		const { sent, fetch } = services();
		const { lines, log } = capture();

		const answer = await exchangeToken('soundcloud', post({ ...code, client_id: 'the-plugins' }), {
			log,
			env,
			fetch,
			flags: flagsOff
		});

		expect(answer.status).toBe(200);
		expect(answer.headers.get('cache-control')).toBe('no-store');
		expect(await answer.json()).toEqual({
			access_token: 'at-1',
			token_type: 'bearer',
			expires_in: 3600,
			refresh_token: 'rt-1'
		});
		expect(sent.map((s) => [s.method, Object.fromEntries(s.form)])).toEqual([
			['POST', { ...code, client_id: 'sc-client', client_secret: 'sc-secret' }]
		]);
		expect(JSON.stringify(lines)).toContain('"service":"soundcloud"');
		expect(SECRETS.filter((secret) => JSON.stringify(lines).includes(secret))).toEqual([]);
	});

	it('refreshes, answering the new refresh token, and passes on what the service refuses with its status', async () => {
		const { sent, fetch } = services();
		const { lines, log } = capture();
		const refresh = (token: string) => post({ grant_type: 'refresh_token', refresh_token: token });

		const fresh = await exchangeToken('soundcloud', refresh('rt-1'), { log, env, fetch, flags: flagsOff });
		expect(await fresh.json()).toMatchObject({ access_token: 'at-2', refresh_token: 'rt-2' });
		expect(Object.fromEntries(sent[0].form)).toEqual({
			grant_type: 'refresh_token',
			refresh_token: 'rt-1',
			client_id: 'sc-client',
			client_secret: 'sc-secret'
		});

		const spent = await exchangeToken('soundcloud', refresh('rt-0'), { log, env, fetch, flags: flagsOff });
		expect([spent.status, await spent.json()]).toEqual([400, { error: 'invalid_grant', error_description: 'spent' }]);

		const wrong = await exchangeToken('soundcloud', refresh('rt-1'), {
			log,
			env: { ...env, SOUNDCLOUD_CLIENT_SECRET: 'stale' },
			fetch,
			flags: flagsOff
		});

		expect([wrong.status, await wrong.json()]).toEqual([401, { error: 'invalid_client' }]);
		expect(SECRETS.filter((secret) => JSON.stringify(lines).includes(secret))).toEqual([]);
	});

	it('asks Mixcloud with a GET and its query, filling in the token type, and never asks it to refresh', async () => {
		const { sent, fetch } = services();
		const { log } = capture();

		const answer = await exchangeToken('mixcloud', post(code), { log, env, fetch, flags: flagsOff });

		expect(await answer.json()).toEqual({ access_token: 'at-1', token_type: 'bearer' });
		expect(sent.map((s) => [s.method, s.url.origin + s.url.pathname, Object.fromEntries(s.form)])).toEqual([
			[
				'GET',
				'https://www.mixcloud.com/oauth/access_token',
				{ client_id: 'mc-client', client_secret: 'mc-secret', redirect_uri: code.redirect_uri, code: 'c0de' }
			]
		]);

		const refused = await exchangeToken('mixcloud', post({ grant_type: 'refresh_token', refresh_token: 'rt-1' }), {
			log,
			env,
			fetch,
			flags: flagsOff
		});

		expect([refused.status, await refused.json()]).toEqual([
			400,
			{ error: 'unsupported_grant_type', error_description: "Mixcloud doesn't refresh tokens" }
		]);

		const unknown = await exchangeToken('mixcloud', post(code), {
			log,
			env: { ...env, MIXCLOUD_CLIENT_ID: 'x' },
			fetch,
			flags: flagsOff
		});

		expect([unknown.status, await unknown.json()]).toEqual([400, { error: 'incorrect_client_credentials' }]);
		expect(sent).toHaveLength(2);
	});

	it.each(['bandcamp', '__proto__', 'constructor', 'toString'])(
		'refuses %s, a service it has no entry for',
		async (id) => {
			const { sent, fetch } = services();

			const answer = await exchangeToken(id, post(code), { log: capture().log, env, fetch, flags: flagsOff });

			expect([answer.status, await answer.json()]).toEqual([
				404,
				{ error: 'invalid_request', error_description: `Local Network doesn't sign in to "${id}"` }
			]);
			expect(sent).toEqual([]);
		}
	);

	it("says it isn't set up for a service whose client id or secret isn't in the environment", async () => {
		const { sent, fetch } = services();
		const { lines, log } = capture();

		for (const missing of [{}, { GOOGLE_CLIENT_ID: 'g-client' }, { GOOGLE_CLIENT_ID: 'g', GOOGLE_CLIENT_SECRET: '' }]) {
			const answer = await exchangeToken('google', post(code), { log, env: missing, fetch, flags: flagsOff });

			expect([answer.status, await answer.json()]).toEqual([
				503,
				{ error: 'temporarily_unavailable', error_description: "Local Network isn't set up to sign in to Google yet" }
			]);
			expect(answer.headers.get('cache-control')).toBe('no-store');
		}

		expect(sent).toEqual([]);
		expect(JSON.stringify(lines[0])).toContain('"env":["GOOGLE_CLIENT_ID","GOOGLE_CLIENT_SECRET"]');
	});

	it('answers 503 for a service whose pause flag is on, sending it nothing, while the others still trade', async () => {
		const { sent, fetch } = services();
		const { lines, log } = capture();
		const flags = { ...flagsOff, pauseMixcloudTokenExchange: true };

		const paused = await exchangeToken('mixcloud', post(code), { log, env, fetch, flags });

		expect([paused.status, await paused.json()]).toEqual([
			503,
			{ error: 'temporarily_unavailable', error_description: 'Local Network has paused signing in to Mixcloud' }
		]);
		expect(paused.headers.get('cache-control')).toBe('no-store');
		expect(sent).toEqual([]);
		expect(JSON.stringify(lines)).toContain('"flag":"pauseMixcloudTokenExchange"');

		const trading = await exchangeToken('soundcloud', post(code), { log, env, fetch, flags });

		expect(trading.status).toBe(200);
		expect(sent.map((s) => s.url.origin)).toEqual(['https://secure.soundcloud.com']);
	});

	it.each([
		['not JSON', 'code=c0de'],
		['another grant', JSON.stringify({ grant_type: 'password', username: 'a', password: 'b' })],
		['a code without its redirect_uri', JSON.stringify({ grant_type: 'authorization_code', code: 'c0de' })],
		['an empty refresh token', JSON.stringify({ grant_type: 'refresh_token', refresh_token: '' })]
	])('refuses %s', async (_, body) => {
		const { sent, fetch } = services();

		const answer = await exchangeToken('soundcloud', posted(body), { log: capture().log, env, fetch, flags: flagsOff });

		expect([answer.status, (await answer.json()).error]).toEqual([400, 'invalid_request']);
		expect(sent).toEqual([]);
	});

	it.each(['http://127.0.0.1:47110/callback', 'http://[::1]:5/callback'])(
		'trades a code sent back to %s, locallibrary on this computer',
		async (redirect) => {
			const { sent, fetch } = services();

			const answer = await exchangeToken('mixcloud', post({ ...code, redirect_uri: redirect }), {
				log: capture().log,
				env,
				fetch,
				flags: flagsOff
			});

			expect(answer.status).toBe(200);
			expect(sent.map((s) => s.form.get('redirect_uri'))).toEqual([redirect]);
		}
	);

	it.each([
		'https://attacker.example/cb',
		'https://127.0.0.1:4/callback',
		'http://localhost:4/callback',
		'http://127.0.0.1/callback',
		'http://127.0.0.1:4/',
		'http://127.0.0.1:4/callback/',
		'http://127.0.0.1:4/callback?next=x',
		'http://127.0.0.1:4/callback#x',
		'http://a@127.0.0.1:1/callback',
		'http://127.0.0.1.attacker.example:1/callback',
		'http://0x7f.0.0.1:4/callback',
		'not a url'
	])('refuses a code sent back to %s, sending nothing on', async (redirect) => {
		const { sent, fetch } = services();
		const { lines, log } = capture();

		for (const service of ['mixcloud', 'soundcloud']) {
			const answer = await exchangeToken(service, post({ ...code, redirect_uri: redirect }), {
				log,
				env,
				fetch,
				flags: flagsOff
			});

			expect([answer.status, await answer.json()]).toEqual([
				400,
				{
					error: 'invalid_request',
					error_description: 'Local Network only trades codes sent back to locallibrary on this computer'
				}
			]);
		}

		expect(sent).toEqual([]);
		expect(JSON.stringify(lines)).not.toContain(redirect);
	});

	it("refuses a refresh whose redirect_uri is somewhere else, and refreshes with locallibrary's", async () => {
		const { sent, fetch } = services();
		const { log } = capture();

		const refresh = (redirect: string) =>
			post({ grant_type: 'refresh_token', refresh_token: 'rt-1', redirect_uri: redirect });

		const elsewhere = await exchangeToken('soundcloud', refresh('https://attacker.example/cb'), {
			log,
			env,
			fetch,
			flags: flagsOff
		});

		expect(elsewhere.status).toBe(400);
		expect(sent).toEqual([]);

		const home = await exchangeToken('soundcloud', refresh('http://127.0.0.1:47110/callback'), {
			log,
			env,
			fetch,
			flags: flagsOff
		});

		expect(await home.json()).toMatchObject({ access_token: 'at-2' });
	});

	it('reads no more of a request than a token request needs', async () => {
		const { sent, fetch } = services();
		const huge = post({ ...code, code: 'c'.repeat(70_000) });

		const answer = await exchangeToken('soundcloud', huge, { log: capture().log, env, fetch, flags: flagsOff });

		expect([answer.status, (await answer.json()).error_description]).toEqual([413, 'The token request is too large']);
		expect(sent).toEqual([]);
	});

	it("says when the service can't be reached or answers what isn't a token response, logging no secret", async () => {
		const { lines, log } = capture();

		// The address Mixcloud is asked at holds the code and secret, and an error may carry it.
		const down = async (input: RequestInfo | URL) => {
			const cause = new Error('connect ECONNREFUSED 127.0.0.1:443');
			throw Object.assign(new TypeError('fetch failed', { cause }), { request: new Request(input).url });
		};

		const unreachable = await exchangeToken('mixcloud', post(code), { log, env, fetch: down, flags: flagsOff });
		expect([unreachable.status, await unreachable.json()]).toEqual([
			502,
			{ error: 'temporarily_unavailable', error_description: "Mixcloud isn't answering" }
		]);
		expect(JSON.stringify(lines)).toContain('fetch failed: connect ECONNREFUSED');

		const slow = async () => {
			throw new DOMException('The operation was aborted due to timeout', 'TimeoutError');
		};

		const late = await exchangeToken('soundcloud', post(code), { log, env, fetch: slow, flags: flagsOff });

		expect(late.status).toBe(502);
		expect(JSON.stringify(lines)).toContain('"reason":"The operation was aborted due to timeout"');

		const thrown = async () => {
			throw 'mc-secret';
		};

		expect((await exchangeToken('mixcloud', post(code), { log, env, fetch: thrown, flags: flagsOff })).status).toBe(
			502
		);
		expect(JSON.stringify(lines)).toContain('"reason":"unknown"');

		// The last is nested deeper than answering it could stringify, though it's well under the size cap.
		const deep = `{"a":${'['.repeat(30_000)}${']'.repeat(30_000)}}`;

		for (const body of [
			'<html>maintenance</html>',
			JSON.stringify({ a: 'b'.repeat(70_000) }),
			'["at-1"]',
			null,
			deep
		]) {
			const odd = async () => new Response(body, { status: body === null ? 502 : 200 });
			const answer = await exchangeToken('soundcloud', post(code), { log, env, fetch: odd, flags: flagsOff });

			expect([answer.status, (await answer.json()).error_description]).toEqual([
				502,
				"SoundCloud answered something that isn't a token response"
			]);
		}

		expect(SECRETS.filter((secret) => JSON.stringify(lines).includes(secret))).toEqual([]);
	});
});

describe('exchangeToken, for anything sent to it', () => {
	const everywhere = { ...env, GOOGLE_CLIENT_ID: 'g-client', GOOGLE_CLIENT_SECRET: 'g-secret' };

	// Keys every object has, which a lookup by id must not mistake for a service, besides any string.
	const ids = fc.oneof(
		{ arbitrary: fc.constantFrom(...Object.keys(registered)), weight: 3 },
		fc.constantFrom('constructor', '__proto__', 'toString', 'hasOwnProperty'),
		fc.string({ maxLength: 12 })
	);

	/** Addresses a code could be sent back to: locallibrary's listener, ones a character off it, and anywhere else. */
	const redirect = fc.oneof(
		fc
			.tuple(
				fc.constantFrom('127.0.0.1', '[::1]', 'localhost', '127.0.0.2', '[::2]'),
				fc.integer({ min: -1, max: 70_000 })
			)
			.chain(([host, port]) =>
				fc
					.constantFrom('', '/', '?x=1', '#x', '/x', '%2F', '@evil.example', '.evil.example')
					.map((suffix) => `http://${host}${port < 0 ? '' : `:${port}`}/callback${suffix}`)
			),
		fc.constantFrom('https://127.0.0.1:4/callback', 'HTTP://127.0.0.1:4/callback', 'http://a@127.0.0.1:4/callback'),
		fc.webUrl(),
		fc.string()
	);

	const grant = fc.oneof(
		remoteObject({
			grant_type: fc.oneof(fc.constantFrom('authorization_code', 'refresh_token', 'password'), fc.string()),
			code: fc.oneof(fc.string({ minLength: 1 }), fc.jsonValue()),
			refresh_token: fc.oneof(fc.string({ minLength: 1 }), fc.jsonValue()),
			code_verifier: fc.string(),
			redirect_uri: redirect,
			client_id: fc.string()
		}),
		// A grant locallibrary could have sent but for its redirect, so the redirect check decides most of these.
		fc
			.record({
				grant_type: fc.constantFrom('authorization_code', 'refresh_token'),
				code: fc.string({ minLength: 1, maxLength: 16 }),
				refresh_token: fc.string({ minLength: 1, maxLength: 16 }),
				redirect_uri: redirect
			})
			.map((value) => JSON.stringify(value))
	);

	const LOOPBACK_CALLBACK = /^http:\/\/(127\.0\.0\.1|\[::1\]):\d+\/callback$/;

	it('answers a token or a 4xx, and sends only a loopback redirect, only to the registered token endpoint', async () => {
		await fc.assert(
			fc.asyncProperty(ids, grant, async (id, body) => {
				const sent: Request[] = [];

				const fetch = async (input: RequestInfo | URL) => {
					sent.push(new Request(input));

					return Response.json({ access_token: 'at-1', token_type: 'bearer' });
				};

				const answer = await exchangeToken(id, posted(body), {
					log: silentLogger,
					env: everywhere,
					fetch,
					flags: flagsOff
				});

				expect([200, 400, 404, 413]).toContain(answer.status);
				// Only an id no service is registered under, "constructor" and "__proto__" among them, is a 404.
				expect(answer.status === 404).toBe(!Object.hasOwn(registered, id));
				expect(answer.headers.get('cache-control')).toBe('no-store');

				const offLoopback: string[] = [];

				for (const request of sent) {
					const url = new URL(request.url);
					const form = request.method === 'POST' ? new URLSearchParams(await request.text()) : url.searchParams;
					const redirectUri = form.get('redirect_uri');

					expect(`${url.origin}${url.pathname}`).toBe(registered[id].tokenUrl);

					if (redirectUri !== null && !LOOPBACK_CALLBACK.test(redirectUri)) offLoopback.push(redirectUri);
				}

				expect(offLoopback).toEqual([]);

				expect(sent.length).toBe(answer.status === 200 ? 1 : 0);
			})
		);
	});

	/** Statuses whose answers have no body. */
	const BODILESS = new Set([204, 205, 304]);

	/** A token answer with one field nested up to 30,000 arrays deep, which still fits under the 64 KiB read cap. */
	const deepAnswer = fc
		.integer({ min: 1, max: 30_000 })
		.map((depth) => `{"access_token":"t","nested":${'['.repeat(depth)}${']'.repeat(depth)}}`);

	it("passes on any service's JSON object answer with its status, and answers 502 for anything else", async () => {
		await fc.assert(
			fc.asyncProperty(
				fc.constantFrom('soundcloud', 'mixcloud'),
				fc.integer({ min: 200, max: 599 }),
				fc.oneof(
					remoteObject({ access_token: fc.oneof(fc.string(), fc.jsonValue()), token_type: fc.jsonValue() }),
					deepAnswer
				),
				fc.constantFrom('application/json', 'text/html', 'application/octet-stream'),
				async (id, status, text, type) => {
					const body = BODILESS.has(status) ? null : text;
					const fetch = async () => new Response(body, { status, headers: { 'content-type': type } });

					const answer = await exchangeToken(id, post(code), { log: silentLogger, env, fetch, flags: flagsOff });
					const answered: unknown = await answer.json();

					expect(answered).toBeTypeOf('object');
					expect(answered).not.toBeNull();
					expect(answer.headers.get('cache-control')).toBe('no-store');
					expect([status, 502]).toContain(answer.status);
				}
			)
		);
	});
});
