import { error } from '@sveltejs/kit';
import { AVATAR_TYPES, imageResponse } from '#lib/server/images.ts';
import type { RequestHandler } from './$types';

const CID = /^[a-z2-7]{20,100}$/;

/**
 * Serves public avatars through our origin (CIDs are immutable, so responses cache forever): project avatars from the
 * root account's PDS, and a person's profile avatar from theirs, only for the CID their indexed profile names as its
 * avatar. Their PDS comes from their DID document, which anyone can publish, so that request goes through the identity
 * resolver's guarded fetch: no redirects, and no private or loopback address, checked as it connects.
 */
export const GET: RequestHandler = async ({ params, locals, fetch }) => {
	const { app } = locals;
	const { did, cid } = params;
	const isRoot = did === app.rootDid;

	if (!CID.test(cid)) error(404, 'Not found');

	if (!isRoot) {
		const profile = await app.profiles.get(did).catch(() => null);

		if (profile?.record.basics.avatar?.ref.$link !== cid) error(404, 'Not found');
	}

	const { pds } = await app.identity.resolve(did);
	// Blobs come from the PDS until HappyView serves them.
	// ast-grep-ignore: no-direct-pds-read
	const url = new URL('/xrpc/com.atproto.sync.getBlob', pds);
	url.searchParams.set('did', did);
	url.searchParams.set('cid', cid);
	const signal = AbortSignal.timeout(15_000);

	// The root's PDS is our own.
	// ast-grep-ignore: no-unguarded-fetch
	const request = isRoot ? fetch(url, { signal }) : app.identity.fetch(url, { redirect: 'error', signal });

	const upstream = await request.catch((cause: unknown) => {
		locals.log.warn({ err: cause, did }, "couldn't fetch an avatar from an account's PDS");
		error(404, 'Not found');
	});

	if (!upstream.ok || !upstream.body) error(upstream.status === 404 ? 404 : 502, 'Image unavailable');
	const type = upstream.headers.get('content-type') ?? 'application/octet-stream';

	return imageResponse(upstream.body, type, AVATAR_TYPES, {
		cacheControl: 'public, max-age=31536000, immutable',
		log: locals.log
	});
};
