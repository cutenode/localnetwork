import { existsSync } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gunzipSync } from 'node:zlib';
import type { RequestEvent } from '@sveltejs/kit';
import type { CaughtError, HandleServerError, ResolveOptions } from '@sveltejs/kit/hooks';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { SESSION_COOKIE, WebSessions } from '#lib/server/auth/session.ts';
import { CRAWLER_HEAD } from '#lib/server/crawler-head.ts';
import { PREVIEW_COOKIE, PREVIEW_MS, sealPreview } from '#lib/server/flags/preview.ts';
import { HappyViewError } from '#lib/server/happyview/client.ts';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { type WrittenLine, writtenLog } from '#lib/server/testing/written-log.ts';
import type { Logger } from '#lib/server/logger.ts';
import type { Viewer } from '#lib/types.ts';
import { flagsOff } from '#lib/testing/flags.ts';
import { routeEvent, type RouteEventInit } from '#lib/testing/route-event.ts';
import type { App as ServerApp } from '#lib/server/app.ts';
import { init, serverHooks } from './hooks.server';
import { FlagService } from '#lib/server/services/flags.ts';

let dir: string;

beforeEach(async () => {
	dir = await mkdtemp(join(tmpdir(), 'hooks-server-test-'));
});

afterEach(async () => {
	vi.unstubAllEnvs();
	await rm(dir, { recursive: true, force: true });
});

it("doesn't start the app under test", async () => {
	// A configuration that gets as far as opening the database, then fails without leaving the machine.
	const databasePath = join(dir, 'app.db');
	vi.stubEnv('DATABASE_PATH', databasePath);
	vi.stubEnv('ROOT_IDENTIFIER', 'did:plc:hooks');
	vi.stubEnv('ROOT_PASSWORD', 'secret');
	vi.stubEnv('PLC_URL', 'http://127.0.0.1:9');
	vi.stubEnv('HAPPYVIEW_URL', 'http://127.0.0.1:9');
	vi.stubEnv('GEOCODER', 'none');

	await init();

	expect(existsSync(databasePath)).toBe(false);
});

it('starts the app outside tests, and carries on when it fails to', async () => {
	vi.stubEnv('NODE_ENV', 'production');
	const logs: WrittenLine[] = [];

	const hooks = serverHooks({
		app: async () => {
			throw new Error('no root account');
		},
		logger: capturingLogger(logs)
	});

	await expect(hooks.init()).resolves.toBeUndefined();
	expect(logs).toEqual([
		expect.objectContaining({
			level: 'error',
			message: 'Local Network could not start; requests will show the setup page'
		})
	]);
});

it("answers a tool's own host without the app", async () => {
	const { handle } = hooksWith(async () => {
		throw new Error('not configured');
	});

	const response = await handled(handle, { url: '/', headers: { host: 'locallibrary.localnetwork.computer' } });

	expect(response.status).toBe(308);
	expect(response.headers.get('location')).toBe('https://localnetwork.computer/tools/locallibrary');
});

it("shows the setup page when the app can't start, with the reason outside production", async () => {
	const { handle } = hooksWith(async () => {
		throw new Error('ROOT_PASSWORD <missing>');
	});

	const response = await handled(handle, { url: '/' });

	expect(response.status).toBe(503);
	expect(response.headers.get('retry-after')).toBe('30');
	expect(await response.text()).toContain('<pre>ROOT_PASSWORD &lt;missing&gt;</pre>');
});

it('keeps the reason for the logs in production, and reports a thrown non-error as text', async () => {
	vi.stubEnv('NODE_ENV', 'production');

	const { handle } = hooksWith(() => Promise.reject('ROOT_PASSWORD missing'));

	const page = await (await handled(handle, { url: '/' })).text();

	expect(page).toContain('<pre>See the server logs.</pre>');
	expect(page).not.toContain('ROOT_PASSWORD');

	vi.stubEnv('NODE_ENV', 'development');

	expect(await (await handled(handle, { url: '/' })).text()).toContain('<pre>ROOT_PASSWORD missing</pre>');
});

it('signs the viewer in from their session cookie, and sets the headers every page gets', async () => {
	const s = await services();
	const token = await s.sessions.create(DJ, null);
	const { handle } = hooksWith(async () => s.app);

	const { response, locals } = await handledWithLocals(handle, {
		url: '/artists',
		routeId: '/artists',
		cookies: { [SESSION_COOKIE]: token },
		headers: { 'x-request-id': 'req-1' }
	});

	expect(locals.viewer?.did).toBe(DJ);
	expect(locals.reauth).toBe(false);
	expect(locals.sessionToken).toBe(token);
	expect(Object.fromEntries(response.headers)).toMatchObject({
		'x-request-id': 'req-1',
		'x-content-type-options': 'nosniff',
		'referrer-policy': 'strict-origin-when-cross-origin',
		'permissions-policy': 'camera=(), microphone=(), geolocation=(self), payment=()'
	});
	expect(response.headers.has('x-robots-tag')).toBe(false);
	expect(response.headers.has('strict-transport-security')).toBe(false);
	expect(s.logs).toEqual([
		expect.objectContaining({
			level: 'info',
			requestId: 'req-1',
			method: 'GET',
			path: '/artists',
			status: 200,
			did: DJ
		})
	]);
});

it('keeps a stale grant signed out with its cookie, so the login page can say why', async () => {
	const s = await services();
	const token = await s.sessions.create(STALE, null);
	const { handle } = hooksWith(async () => s.app);

	const { locals, cookies } = await handledWithLocals(handle, {
		url: '/artists',
		cookies: { [SESSION_COOKIE]: token }
	});

	expect(locals.viewer).toBeNull();
	expect(locals.reauth).toBe(true);
	expect(cookies.get(SESSION_COOKIE)).toBe(token);
});

it("applies a staff member's flag preview to their own requests only", async () => {
	const s = await services({ admins: [ADMIN] });
	const { handle } = hooksWith(async () => s.app);
	const adminToken = await s.sessions.create(ADMIN, null);
	const djToken = await s.sessions.create(DJ, null);

	const preview = (did: string, token: string) =>
		sealPreview({ did, flags: { parityCanary: true }, expiresAt: Date.now() + PREVIEW_MS }, token);

	const admin = await handledWithLocals(handle, {
		url: '/',
		cookies: { [SESSION_COOKIE]: adminToken, [PREVIEW_COOKIE]: preview(ADMIN, adminToken) }
	});

	expect(admin.locals.flags).toEqual({ ...flagsOff, parityCanary: true });
	expect(admin.locals.flagPreview).toEqual({ parityCanary: true });

	// The admin's cookie, copied into someone else's browser, and one they signed for themselves.
	for (const cookie of [preview(ADMIN, adminToken), preview(DJ, djToken)]) {
		const dj = await handledWithLocals(handle, {
			url: '/',
			cookies: { [SESSION_COOKIE]: djToken, [PREVIEW_COOKIE]: cookie }
		});

		expect(dj.locals.flags).toEqual(flagsOff);
		expect(dj.locals.flagPreview).toBeUndefined();
		expect(dj.cookies.deleted).toContain(PREVIEW_COOKIE);
	}

	const signedOut = await handledWithLocals(handle, {
		url: '/',
		cookies: { [PREVIEW_COOKIE]: preview(ADMIN, adminToken) }
	});

	expect(signedOut.locals.flags).toEqual(flagsOff);
	expect(signedOut.cookies.deleted).toContain(PREVIEW_COOKIE);
});

it('drops a session cookie that names no session', async () => {
	const s = await services();
	const { handle } = hooksWith(async () => s.app);

	const { locals, cookies } = await handledWithLocals(handle, {
		url: '/artists',
		cookies: { [SESSION_COOKIE]: 'x'.repeat(43) }
	});

	expect(locals.viewer).toBeNull();
	expect(cookies.deleted).toContain(SESSION_COOKIE);
});

it("doesn't read the session cookie on an xrpc call, or rate limit it", async () => {
	const s = await services({ writes: 0 });
	const token = await s.sessions.create(DJ, null);
	const { handle } = hooksWith(async () => s.app);

	const { response, locals } = await handledWithLocals(handle, {
		url: '/xrpc/com.atproto.space.notifyWrite',
		request: new Request('https://localnetwork.test/xrpc/com.atproto.space.notifyWrite', { method: 'POST' }),
		cookies: { [SESSION_COOKIE]: token }
	});

	expect(response.status).toBe(200);
	expect(locals.viewer).toBeNull();
});

it("gives an xrpc call a signed-out visitor's flags, and ignores a staff preview there", async () => {
	const s = await services({ admins: [ADMIN], flagOverrides: { parityCanary: true } });
	const token = await s.sessions.create(ADMIN, null);
	const resolve = vi.spyOn(s.sessions, 'resolve');
	const { handle } = hooksWith(async () => s.app);

	const preview = sealPreview(
		{ did: ADMIN, flags: { parityCanary: false }, expiresAt: Date.now() + PREVIEW_MS },
		token
	);

	const { locals, cookies } = await handledWithLocals(handle, {
		url: '/xrpc/computer.localnetwork.flags.getFlags',
		cookies: { [SESSION_COOKIE]: token, [PREVIEW_COOKIE]: preview }
	});

	expect(locals.flags?.parityCanary).toBe(true);
	expect(locals.flagPreview).toBeUndefined();
	expect(cookies.deleted).not.toContain(PREVIEW_COOKIE);
	expect(resolve).not.toHaveBeenCalled();
});

it('limits writes by address, and counts an unknown address as one', async () => {
	const s = await services({ writes: 1 });
	const { handle } = hooksWith(async () => s.app);
	const post = () => new Request('https://localnetwork.test/report', { method: 'POST' });

	expect((await handled(handle, { url: '/report', request: post() })).status).toBe(200);
	expect((await handled(handle, { url: '/report', request: post(), clientAddress: '198.51.100.1' })).status).toBe(200);

	const limited = await handled(handle, { url: '/report', request: post() });

	expect(limited.status).toBe(429);
	expect(limited.headers.get('retry-after')).toBe('30');

	// Without an address (adapter-node behind no proxy can't always tell), every such request shares one bucket.
	const unknown = await handledWithoutAddress(handle, post());

	expect(unknown.status).toBe(200);
	expect((await handledWithoutAddress(handle, post())).status).toBe(429);
});

it('sends someone signed out away from the dashboard before the page loads', async () => {
	const s = await services();
	const { handle } = hooksWith(async () => s.app);

	await expect(handled(handle, { url: '/dashboard?tab=1', routeId: '/dashboard' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%3Ftab%3D1'
	});
});

it('keeps unlisted pages out of search, and sends HSTS on https', async () => {
	const s = await services({ publicUrl: 'https://localnetwork.computer' });
	const token = await s.sessions.create(DJ, null);
	const { handle } = hooksWith(async () => s.app);

	const response = await handled(handle, {
		url: '/dashboard',
		routeId: '/dashboard',
		cookies: { [SESSION_COOKIE]: token }
	});

	expect(response.headers.get('x-robots-tag')).toBe('noindex');
	expect(response.headers.get('strict-transport-security')).toBe('max-age=31536000; includeSubDomains');
});

it('logs failures by how bad they are, and leaves assets and health checks out', async () => {
	const s = await services();
	const { handle } = hooksWith(async () => s.app);

	await handled(handle, { url: '/missing' }, 404);
	await handled(handle, { url: '/broken' }, 500);
	await handled(handle, { url: '/_app/immutable/app.js' });
	await handled(handle, { url: '/healthz' });

	expect(s.logs.map(({ level, path }) => [level, path])).toEqual([
		['warn', '/missing'],
		['error', '/broken']
	]);
});

it.each([
	['/dashboard/activity?before=4', '/dashboard/account/activity?before=4'],
	['/dashboard/data', '/dashboard/account/data'],
	['/dashboard/requests', '/dashboard/account/requests'],
	['/dashboard/requests/3k?submitted=1', '/dashboard/account/requests/3k?submitted=1'],
	['/dashboard/requests/3k/avatar?size=64', '/dashboard/account/requests/3k/avatar?size=64'],
	['/dashboard/inbox/settings?email=declined', '/dashboard/account/notifications?email=declined']
])('sends %s on to where it moved, whatever the method', async (from, to) => {
	const { handle } = hooksWith(async () => {
		throw new Error('a moved page needs nothing from the app');
	});

	for (const request of [{ url: from }, { url: from, form: {} }]) {
		const { event } = routeEvent(request);

		// SAFETY: as above.
		await expect(handle({ event: event as never, resolve: async () => new Response() })).rejects.toMatchObject({
			status: 308,
			location: to
		});
	}
});

it.each(['/dashboard/inbox', '/dashboard/requests-archive', '/dashboard/account/data'])(
	'leaves %s where it is',
	async (url) => {
		const s = await services();
		const { handle } = hooksWith(async () => s.app);
		const { event } = routeEvent({ url });

		// SAFETY: as above.
		const response = await handle({ event: event as never, resolve: async () => new Response('here') });

		expect(await response.text()).toBe('here');
	}
);

it('puts what services note during a request on its one line', async () => {
	const s = await services();
	const { handle } = hooksWith(async () => s.app);
	const { event } = routeEvent({ url: '/dashboard/spaces' });

	const resolve = async () => {
		s.app.logger.child({ component: 'spaces' }).warn('could not read a space');

		return new Response('ok');
	};

	// SAFETY: as above.
	await handle({ event: event as never, resolve });

	expect(s.logs).toEqual([
		expect.objectContaining({
			level: 'warn',
			message: 'could not read a space',
			path: '/dashboard/spaces',
			status: 200,
			notes: [{ level: 'warn', message: 'could not read a space', component: 'spaces' }]
		})
	]);
});

it('logs a request the write limit turns away', async () => {
	const s = await services({ writes: 0 });
	const { handle } = hooksWith(async () => s.app);

	await handled(handle, {
		url: '/report',
		request: new Request('https://localnetwork.test/report', { method: 'POST' })
	});

	expect(s.logs.map(({ level, status }) => [level, status])).toEqual([['warn', 429]]);
});

it('makes up a request id when the request brings none, and cuts a long one short', async () => {
	const s = await services();
	const { handle } = hooksWith(async () => s.app);

	const made = await handled(handle, { url: '/' });
	const long = await handled(handle, { url: '/', headers: { 'x-request-id': 'r'.repeat(100) } });

	expect(made.headers.get('x-request-id')).toMatch(/^[0-9a-f-]{36}$/);
	expect(long.headers.get('x-request-id')).toBe('r'.repeat(64));
});

it("writes the page's crawler tags into the head, once, and gzips the page", async () => {
	const s = await services();
	const { handle } = hooksWith(async () => s.app);
	const chunks = [`<head>${CRAWLER_HEAD}</head>`, `<body>${CRAWLER_HEAD}</body>`];

	const resolve = async (event: RequestEvent, options?: ResolveOptions) => {
		event.locals.crawlerHead = '<meta name="description" content="DJs">';

		let html = '';

		for (const [index, chunk] of chunks.entries())
			html += (await options!.transformPageChunk!({ html: chunk, done: index === chunks.length - 1 })) ?? '';

		return new Response(html, { headers: { 'content-type': 'text/html' } });
	};

	const { event } = routeEvent({ url: '/', headers: { 'accept-encoding': 'gzip' } });
	// SAFETY: the hook reads the request, URL, route, cookies and client address, all set by routeEvent.
	const response = await handle({ event: event as never, resolve });

	expect(response.headers.get('content-encoding')).toBe('gzip');
	expect(gunzipSync(Buffer.from(await response.arrayBuffer())).toString()).toBe(
		`<head><meta name="description" content="DJs"></head><body>${CRAWLER_HEAD}</body>`
	);
});

it("preloads the scripts, styles and the two typefaces' Latin files, and nothing else", async () => {
	const s = await services();
	const { handle } = hooksWith(async () => s.app);
	let preload: ResolveOptions['preload'];

	const resolve = async (_event: RequestEvent, options?: ResolveOptions) => {
		preload = options?.preload;

		return new Response('ok');
	};

	const { event } = routeEvent({ url: '/' });
	// SAFETY: as above.
	await handle({ event: event as never, resolve });

	const font = (filename: string) => preload!({ type: 'font', path: `/_app/${filename}`, filename });

	expect(preload!({ type: 'js', path: '/_app/start.js' })).toBe(true);
	expect(preload!({ type: 'css', path: '/_app/app.css' })).toBe(true);
	expect(font('instrument-sans-latin-wght-normal.B1x2.woff2')).toBe(true);
	expect(font('jetbrains-mono-latin-wght-normal.C3y4.woff2')).toBe(true);
	expect(font('instrument-sans-cyrillic-wght-normal.D5z6.woff2')).toBe(false);
	expect(preload!({ type: 'asset', path: '/_app/hero.avif' })).toBe(false);
});

it("tells the visitor the directory is down when HappyView doesn't answer, and hides other errors", async () => {
	const { handleError } = hooksWith(async () => ({}));
	const logs: WrittenLine[] = [];
	const locals = { requestId: 'req-2', log: capturingLogger(logs) };

	const directory = handleError(errorInput({ kind: 'unknown', error: new HappyViewError(502, 'bad gateway') }, locals));
	const unknown = handleError(errorInput({ kind: 'unknown', error: new Error('boom') }, locals));

	expect(directory).toEqual({
		status: 503,
		message: "The DJ directory isn't answering right now. Try again in a minute.",
		requestId: 'req-2'
	});
	expect(unknown).toEqual({ status: 500, message: 'Something went wrong on our side.', requestId: 'req-2' });
	expect(logs.flatMap(({ notes }) => notes!.map(({ level, path, requestId }) => [level, path, requestId]))).toEqual([
		['error', '/broken', 'req-2'],
		['error', '/broken', 'req-2']
	]);
});

it("passes on the status and message of SvelteKit's own errors, with a request id even before the hook set one", async () => {
	const logs: WrittenLine[] = [];
	const { handleError } = serverHooks({ app: async () => app({}), logger: capturingLogger(logs) });

	const expected = handleError(errorInput({ kind: 'framework', error: { status: 404, message: 'Not Found' } }, {}));

	const unknown = handleError(errorInput({ kind: 'unknown', error: new Error('boom') }, {}));

	expect(expected).toEqual({ status: 404, message: 'Not Found', requestId: expect.stringMatching(/^[0-9a-f-]{36}$/) });
	expect(unknown).toMatchObject({ status: 500, requestId: expect.stringMatching(/^[0-9a-f-]{36}$/) });
	// Before the hook gave the request the app's logger, the error goes to the hooks' own.
	expect(logs.flatMap(({ notes }) => notes!.map(({ level, path }) => [level, path]))).toEqual([['error', '/broken']]);
});

// Helpers

const DJ = 'did:plc:dj';

const ADMIN = 'did:plc:admin';

/** An account whose OAuth grant lacks scopes the app now asks for, so the app has no viewer for it. */
const STALE = 'did:plc:stale';

type Hooks = ReturnType<typeof serverHooks>;

/** The app as far as the hooks read it: each test gives only the parts its requests reach. */
type HookApp = {
	logger?: Logger;
	sessions?: WebSessions;
	limits?: { write: RateLimiter };
	flags?: Pick<FlagService, 'evaluate' | 'preview'>;
	viewer?: (did: string) => Promise<Viewer | null>;
	config?: { publicUrl: string };
};

// SAFETY: the hooks read only the parts of the app a test's requests reach, and each test gives those.
const app = (parts: HookApp): ServerApp => parts as never;

/** The hooks over an app that `load` gives (or fails to), with their own logger quiet. */
function hooksWith(load: () => Promise<HookApp>): Hooks {
	return serverHooks({ app: async () => app(await load()), logger: capturingLogger([]) });
}

/** A logger whose lines land in `logs`. */
function capturingLogger(logs: WrittenLine[]) {
	return writtenLog('info', logs).logger;
}

/** The app parts the hook reads: real sessions and viewers over test services, a write limit of `writes`, and flags with `flagOverrides` forced. */
async function services(
	options: { writes?: number; publicUrl?: string; admins?: string[]; flagOverrides?: Record<string, boolean> } = {}
) {
	const t = await createTestServices({ admins: options.admins });
	const sessions = new WebSessions(t.db, 30);
	const logs: WrittenLine[] = [];

	const parts = {
		logger: capturingLogger(logs),
		sessions,
		limits: { write: new RateLimiter(options.writes ?? 30, 0) },
		flags: options.flagOverrides ? new FlagService(t.core, t.staff, options.flagOverrides) : t.flags,
		viewer: async (did: string) => (did === STALE ? null : t.viewer(did)),
		config: { publicUrl: options.publicUrl ?? 'http://127.0.0.1:5173' }
	};

	return { app: parts, sessions, logs };
}

/** Runs the handle hook on a request built from `init`, with a page that answers `status`. */
async function handledWithLocals(handle: Hooks['handle'], request: RouteEventInit, status = 200) {
	const { event, cookies } = routeEvent(request);
	// The hook sets these itself, so they start out unset as SvelteKit leaves them.
	const locals: Partial<App.Locals> = {};
	const resolve = async () => new Response('page', { status, headers: { 'content-type': 'text/plain' } });

	// SAFETY: the hook reads the request, URL, route, cookies and client address, all set by routeEvent.
	const response = await handle({ event: { ...event, locals } as never, resolve });

	return { response, locals, cookies };
}

async function handled(handle: Hooks['handle'], request: RouteEventInit, status = 200) {
	return (await handledWithLocals(handle, request, status)).response;
}

/** A write whose client address SvelteKit can't tell, as `getClientAddress` throws then. */
async function handledWithoutAddress(handle: Hooks['handle'], request: Request) {
	const { event } = routeEvent({ url: new URL(request.url).pathname, request });

	const getClientAddress = () => {
		throw new Error('Could not determine clientAddress');
	};

	// SAFETY: as in handledWithLocals.
	return handle({
		event: { ...event, locals: {}, getClientAddress } as never,
		resolve: async () => new Response('page')
	});
}

/** handleError's input for an error `caught` on /broken, with `locals` as the hook left them. */
function errorInput(caught: CaughtError, locals: Partial<App.Locals>): Parameters<HandleServerError>[0] {
	const { event } = routeEvent({ url: '/broken' });

	// SAFETY: handleError reads only the event's URL and locals, and the error.
	return { ...caught, event: { ...event, locals } as never };
}
