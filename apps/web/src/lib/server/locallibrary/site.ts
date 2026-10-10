// The site for locallibrary, the DJ drive tool (CLI and terminal player) that signs in with
// Atmosphere accounts as `https://locallibrary.localnetwork.computer/oauth-client-metadata.json`.
// This app serves it to requests for that host. It can't share the main site's
// /oauth-client-metadata.json: that one is Local Network's own web client, and a native app needs
// a client with a 127.0.0.1 redirect.
//
// oauth-client-metadata.json is written by locallibrary:
//   locallibrary account client-metadata -o src/lib/server/locallibrary/oauth-client-metadata.json
import clientMetadata from './oauth-client-metadata.json?raw';
import page from './index.html?raw';

const DEFAULT_HOST = 'locallibrary.localnetwork.computer';

/** The host locallibrary's site answers on (`LOCALLIBRARY_HOST` overrides it, e.g. for staging). */
export const locallibraryHost = (): string => (process.env.LOCALLIBRARY_HOST || DEFAULT_HOST).toLowerCase();

/** The host a request was made to, without its port. A proxy's `x-forwarded-host` wins. */
function requestHost(request: Request): string {
	const forwarded = request.headers.get('x-forwarded-host')?.split(',')[0]?.trim();
	const host = forwarded || request.headers.get('host') || '';

	return host.replace(/:\d+$/, '').toLowerCase();
}

const common = {
	'x-content-type-options': 'nosniff',
	'referrer-policy': 'strict-origin-when-cross-origin'
};

/**
 * locallibrary's site, for requests to its host: the client document servers fetch while signing
 * in, and a page about locallibrary. `null` for every other host, which the app serves as usual.
 */
export function locallibrarySite(request: Request, pathname: string): Response | null {
	if (requestHost(request) !== locallibraryHost()) return null;

	if (request.method !== 'GET' && request.method !== 'HEAD') {
		return new Response('Method not allowed', { status: 405, headers: { ...common, allow: 'GET, HEAD' } });
	}

	const body = (text: string) => (request.method === 'HEAD' ? null : text);

	if (pathname === '/oauth-client-metadata.json') {
		// Servers fetch this while someone signs in: JSON, a 200, and no redirect.
		return new Response(body(clientMetadata), {
			headers: {
				...common,
				'content-type': 'application/json',
				'cache-control': 'public, max-age=600',
				'access-control-allow-origin': '*'
			}
		});
	}

	if (pathname === '/') {
		return new Response(body(page), {
			headers: {
				...common,
				'content-type': 'text/html; charset=utf-8',
				'cache-control': 'public, max-age=3600',
				'content-security-policy':
					"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
			}
		});
	}

	return new Response('Not found', {
		status: 404,
		headers: { ...common, 'content-type': 'text/plain; charset=utf-8' }
	});
}
