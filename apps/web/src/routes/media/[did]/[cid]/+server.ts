import { error } from '@sveltejs/kit';
import type { RequestHandler } from './$types';

const CID = /^[a-z2-7]{20,100}$/;

/**
 * Serves public project images from the root account's PDS through our
 * origin (CIDs are immutable, so responses cache forever).
 */
export const GET: RequestHandler = async ({ params, locals, fetch }) => {
	const { app } = locals;

	if (params.did !== app.rootDid || !CID.test(params.cid)) error(404, 'Not found');
	const pds = (await app.identity.resolve(app.rootDid)).pds;
	// Blobs come from the PDS until HappyView serves them.
	// ast-grep-ignore: no-direct-pds-read
	const url = new URL('/xrpc/com.atproto.sync.getBlob', pds);
	url.searchParams.set('did', params.did);
	url.searchParams.set('cid', params.cid);
	const upstream = await fetch(url, { signal: AbortSignal.timeout(15_000) });

	if (!upstream.ok || !upstream.body) error(upstream.status === 404 ? 404 : 502, 'Image unavailable');
	const type = upstream.headers.get('content-type') ?? 'application/octet-stream';

	if (!type.startsWith('image/')) error(415, 'Not an image');

	return new Response(upstream.body, {
		headers: {
			'content-type': type,
			'cache-control': 'public, max-age=31536000, immutable',
			'content-security-policy': "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'",
			'x-content-type-options': 'nosniff'
		}
	});
};
