import { randomUUID } from 'node:crypto';
import type { Handle, HandleServerError, ServerInit } from '@sveltejs/kit/hooks';
import { getApp } from '#lib/server/app.ts';
import { HappyViewError } from '#lib/server/happyview/client.ts';
import { SESSION_COOKIE } from '#lib/server/auth/session.ts';
import { gzipped } from '#lib/server/compress.ts';
import { createLogger } from '#lib/server/logger.ts';
import { toolSite } from '#lib/server/tools/site.ts';

const fallbackLogger = createLogger({
	level: process.env.LOG_LEVEL ?? 'info',
	pretty: process.env.NODE_ENV !== 'production'
});

export const init: ServerInit = async () => {
	try {
		await getApp();
	} catch (error) {
		fallbackLogger.error({ err: error }, 'Local Network could not start; requests will show the setup page');
	}
};

const setupPage = (message: string) =>
	`<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Local Network setup</title>
<style>body{font:16px/1.5 system-ui,sans-serif;max-width:44rem;margin:4rem auto;padding:0 1rem;color:#18181b}pre{background:#f4f4f5;padding:1rem;border-radius:.5rem;white-space:pre-wrap}</style></head>
<body><h1>Local Network isn't configured yet</h1><p>The server could not connect its root AT Protocol account. Check your environment variables (see <code>.env.example</code>) or run <code>pnpm dev:network</code> for a local spaces-enabled PDS.</p><pre>${message.replace(/[<>&]/g, (c) => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;' })[c]!)}</pre></body></html>`;

const MUTATING = new Set(['POST', 'PUT', 'PATCH', 'DELETE']);

export const handle: Handle = async ({ event, resolve }) => {
	// The tools' own hosts need nothing from the app, so they answer even while it isn't configured.
	const tool = toolSite(event.request, event.url.pathname);

	if (tool) return gzipped(event.request, tool);

	const started = performance.now();
	const requestId = event.request.headers.get('x-request-id')?.slice(0, 64) || randomUUID();
	event.locals.requestId = requestId;

	let app;

	try {
		app = await getApp();
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);

		return new Response(setupPage(process.env.NODE_ENV === 'production' ? 'See the server logs.' : message), {
			status: 503,
			headers: { 'content-type': 'text/html; charset=utf-8', 'retry-after': '30' }
		});
	}

	event.locals.app = app;
	const log = app.logger.child({ requestId });
	event.locals.log = log;

	const isXrpc = event.url.pathname.startsWith('/xrpc/');

	const clientIp = (() => {
		try {
			return event.getClientAddress();
		} catch {
			return 'unknown';
		}
	})();

	if (MUTATING.has(event.request.method) && !isXrpc && !app.limits.write.take(`ip:${clientIp}`)) {
		return new Response('Too many requests. Slow down a little.', { status: 429, headers: { 'retry-after': '30' } });
	}

	const token = event.cookies.get(SESSION_COOKIE);
	event.locals.sessionToken = token;
	event.locals.viewer = null;

	if (token && !isXrpc) {
		const did = await app.sessions.resolve(token);

		if (did) {
			event.locals.viewer = await app.viewer(did);
			// The cookie stays so the login page can say why. Signing in again replaces it.
			event.locals.reauth = !event.locals.viewer;
		} else {
			event.cookies.delete(SESSION_COOKIE, { path: '/' });
		}
	}

	// The two typefaces set every line and differ in width from the system fallbacks, so arriving after
	// the first paint they moved the whole page (Lighthouse's CLS). Their Latin files are preloaded with
	// the CSS; the other subsets load only for pages that need them.
	const response = gzipped(
		event.request,
		await resolve(event, {
			preload: ({ type, path }) =>
				type === 'js' ||
				type === 'css' ||
				(type === 'font' && /(instrument-sans|jetbrains-mono)-latin-wght-normal\.[\w-]+\.woff2$/.test(path))
		})
	);

	response.headers.set('x-request-id', requestId);
	response.headers.set('x-content-type-options', 'nosniff');
	response.headers.set('referrer-policy', 'strict-origin-when-cross-origin');
	response.headers.set('permissions-policy', 'camera=(), microphone=(), geolocation=(self), payment=()');

	if (app.config.publicUrl.startsWith('https:')) {
		response.headers.set('strict-transport-security', 'max-age=31536000; includeSubDomains');
	}

	const ms = Math.round(performance.now() - started);
	const level = response.status >= 500 ? 'error' : response.status >= 400 ? 'warn' : 'info';

	if (!event.url.pathname.startsWith('/_app/') && event.url.pathname !== '/healthz') {
		log[level](
			{
				method: event.request.method,
				path: event.url.pathname,
				status: response.status,
				ms,
				did: event.locals.viewer?.did
			},
			'request'
		);
	}

	return response;
};

export const handleError: HandleServerError = (input) => {
	const { event } = input;
	const requestId = event.locals.requestId ?? randomUUID();

	if (input.kind === 'unknown') {
		(event.locals.log ?? fallbackLogger).error(
			{ err: input.error, path: event.url.pathname, requestId },
			'unhandled error'
		);

		if (input.error instanceof HappyViewError) {
			return { status: 503, message: "The DJ directory isn't answering right now. Try again in a minute.", requestId };
		}

		return { status: 500, message: 'Something went wrong on our side.', requestId };
	}

	return { status: input.error.status, message: input.error.message, requestId };
};
