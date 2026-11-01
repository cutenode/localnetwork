import pino from 'pino';
import { describe, expect, it } from 'vitest';
import { exchangeToken } from './exchange';

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
	const lines: string[] = [];

	return { lines, log: pino({ level: 'debug' }, { write: (line: string) => lines.push(line) }) };
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

		const answer = await exchangeToken('soundcloud', post({ ...code, client_id: 'the-plugins' }), { log, env, fetch });

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
		expect(lines.join('\n')).toContain('"service":"soundcloud"');
		expect(SECRETS.filter((secret) => lines.join('\n').includes(secret))).toEqual([]);
	});

	it('refreshes, answering the new refresh token, and passes on what the service refuses with its status', async () => {
		const { sent, fetch } = services();
		const { lines, log } = capture();
		const refresh = (token: string) => post({ grant_type: 'refresh_token', refresh_token: token });

		const fresh = await exchangeToken('soundcloud', refresh('rt-1'), { log, env, fetch });
		expect(await fresh.json()).toMatchObject({ access_token: 'at-2', refresh_token: 'rt-2' });
		expect(Object.fromEntries(sent[0].form)).toEqual({
			grant_type: 'refresh_token',
			refresh_token: 'rt-1',
			client_id: 'sc-client',
			client_secret: 'sc-secret'
		});

		const spent = await exchangeToken('soundcloud', refresh('rt-0'), { log, env, fetch });
		expect([spent.status, await spent.json()]).toEqual([400, { error: 'invalid_grant', error_description: 'spent' }]);

		const wrong = await exchangeToken('soundcloud', refresh('rt-1'), {
			log,
			env: { ...env, SOUNDCLOUD_CLIENT_SECRET: 'stale' },
			fetch
		});

		expect([wrong.status, await wrong.json()]).toEqual([401, { error: 'invalid_client' }]);
		expect(SECRETS.filter((secret) => lines.join('\n').includes(secret))).toEqual([]);
	});

	it('asks Mixcloud with a GET and its query, filling in the token type, and never asks it to refresh', async () => {
		const { sent, fetch } = services();
		const { log } = capture();

		const answer = await exchangeToken('mixcloud', post(code), { log, env, fetch });

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
			fetch
		});

		expect([refused.status, await refused.json()]).toEqual([
			400,
			{ error: 'unsupported_grant_type', error_description: "Mixcloud doesn't refresh tokens" }
		]);

		const unknown = await exchangeToken('mixcloud', post(code), {
			log,
			env: { ...env, MIXCLOUD_CLIENT_ID: 'x' },
			fetch
		});

		expect([unknown.status, await unknown.json()]).toEqual([400, { error: 'incorrect_client_credentials' }]);
		expect(sent).toHaveLength(2);
	});

	it.each(['bandcamp', '__proto__', 'constructor', 'toString'])(
		'refuses %s, a service it has no entry for',
		async (id) => {
			const { sent, fetch } = services();

			const answer = await exchangeToken(id, post(code), { log: capture().log, env, fetch });

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
			const answer = await exchangeToken('google', post(code), { log, env: missing, fetch });

			expect([answer.status, await answer.json()]).toEqual([
				503,
				{ error: 'temporarily_unavailable', error_description: "Local Network isn't set up to sign in to Google yet" }
			]);
			expect(answer.headers.get('cache-control')).toBe('no-store');
		}

		expect(sent).toEqual([]);
		expect(lines[0]).toContain('"env":["GOOGLE_CLIENT_ID","GOOGLE_CLIENT_SECRET"]');
	});

	it.each([
		['not JSON', 'code=c0de'],
		['another grant', JSON.stringify({ grant_type: 'password', username: 'a', password: 'b' })],
		['a code without its redirect_uri', JSON.stringify({ grant_type: 'authorization_code', code: 'c0de' })],
		['an empty refresh token', JSON.stringify({ grant_type: 'refresh_token', refresh_token: '' })]
	])('refuses %s', async (_, body) => {
		const { sent, fetch } = services();

		const answer = await exchangeToken('soundcloud', posted(body), { log: capture().log, env, fetch });

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
				fetch
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
			const answer = await exchangeToken(service, post({ ...code, redirect_uri: redirect }), { log, env, fetch });

			expect([answer.status, await answer.json()]).toEqual([
				400,
				{
					error: 'invalid_request',
					error_description: 'Local Network only trades codes sent back to locallibrary on this computer'
				}
			]);
		}

		expect(sent).toEqual([]);
		expect(lines.join('\n')).not.toContain(redirect);
	});

	it("refuses a refresh whose redirect_uri is somewhere else, and refreshes with locallibrary's", async () => {
		const { sent, fetch } = services();
		const { log } = capture();

		const refresh = (redirect: string) =>
			post({ grant_type: 'refresh_token', refresh_token: 'rt-1', redirect_uri: redirect });

		const elsewhere = await exchangeToken('soundcloud', refresh('https://attacker.example/cb'), { log, env, fetch });
		expect(elsewhere.status).toBe(400);
		expect(sent).toEqual([]);

		const home = await exchangeToken('soundcloud', refresh('http://127.0.0.1:47110/callback'), { log, env, fetch });
		expect(await home.json()).toMatchObject({ access_token: 'at-2' });
	});

	it('reads no more of a request than a token request needs', async () => {
		const { sent, fetch } = services();
		const huge = post({ ...code, code: 'c'.repeat(70_000) });

		const answer = await exchangeToken('soundcloud', huge, { log: capture().log, env, fetch });

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

		const unreachable = await exchangeToken('mixcloud', post(code), { log, env, fetch: down });
		expect([unreachable.status, await unreachable.json()]).toEqual([
			502,
			{ error: 'temporarily_unavailable', error_description: "Mixcloud isn't answering" }
		]);
		expect(lines.join('\n')).toContain('fetch failed: connect ECONNREFUSED');

		const slow = async () => {
			throw new DOMException('The operation was aborted due to timeout', 'TimeoutError');
		};

		const late = await exchangeToken('soundcloud', post(code), { log, env, fetch: slow });

		expect(late.status).toBe(502);
		expect(lines.join('\n')).toContain('"reason":"The operation was aborted due to timeout"');

		for (const body of ['<html>maintenance</html>', JSON.stringify({ a: 'b'.repeat(70_000) }), '["at-1"]', null]) {
			const odd = async () => new Response(body, { status: body === null ? 502 : 200 });
			const answer = await exchangeToken('soundcloud', post(code), { log, env, fetch: odd });

			expect([answer.status, (await answer.json()).error_description]).toEqual([
				502,
				"SoundCloud answered something that isn't a token response"
			]);
		}

		expect(SECRETS.filter((secret) => lines.join('\n').includes(secret))).toEqual([]);
	});
});
