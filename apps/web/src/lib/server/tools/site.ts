// The tools' own hosts, such as locallibrary.localnetwork.computer. A native tool signs in with
// Atmosphere accounts as `https://<its host>/oauth-client-metadata.json`, so its host serves that
// document. It can't share the main site's /oauth-client-metadata.json: that one is Local
// Network's own web client, and a native app needs a client with a 127.0.0.1 redirect. Everything
// else about a tool lives on the main site, at /tools/<id>.
import { tools } from '#lib/tools/index.ts';

/** Where the main site is when `PUBLIC_URL` isn't set, since this answers before the app is configured. */
const DEFAULT_PUBLIC_URL = 'https://localnetwork.computer';

/** The host a request was made to, without its port. A proxy's `x-forwarded-host` wins. */
function requestHost(request: Request): string {
	const forwarded = request.headers.get('x-forwarded-host')?.split(',')[0]?.trim();
	const host = forwarded || request.headers.get('host') || '';

	return host.replace(/:\d+$/, '').toLowerCase();
}

function publicUrl(): string {
	const configured = process.env.PUBLIC_URL;

	return configured && URL.canParse(configured) ? configured.replace(/\/+$/, '') : DEFAULT_PUBLIC_URL;
}

const common = {
	'x-content-type-options': 'nosniff',
	'referrer-policy': 'strict-origin-when-cross-origin'
};

/**
 * A tool's site, for requests to one of its hosts: the client document servers fetch while
 * signing in, and a redirect from `/` to the tool's page on the main site. `null` for every other
 * host, which the app serves as usual.
 */
export function toolSite(request: Request, pathname: string): Response | null {
	const host = requestHost(request);
	const tool = tools.find((candidate) => candidate.hosts?.().includes(host));

	if (!tool) return null;

	if (request.method !== 'GET' && request.method !== 'HEAD') {
		return new Response('Method not allowed', { status: 405, headers: { ...common, allow: 'GET, HEAD' } });
	}

	if (pathname === '/oauth-client-metadata.json' && tool.clientMetadata) {
		// Servers fetch this while someone signs in: JSON, a 200, and no redirect.
		return new Response(request.method === 'HEAD' ? null : tool.clientMetadata, {
			headers: {
				...common,
				'content-type': 'application/json',
				'cache-control': 'public, max-age=600',
				'access-control-allow-origin': '*'
			}
		});
	}

	if (pathname === '/') {
		return new Response(null, {
			status: 308,
			headers: { ...common, location: `${publicUrl()}/tools/${tool.id}`, 'cache-control': 'public, max-age=3600' }
		});
	}

	return new Response('Not found', {
		status: 404,
		headers: { ...common, 'content-type': 'text/plain; charset=utf-8' }
	});
}
