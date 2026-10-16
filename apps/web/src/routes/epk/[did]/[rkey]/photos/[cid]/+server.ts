import { error } from '@sveltejs/kit';
import { EPK_PHOTO_MAX_BYTES } from '#lib/server/forms/epk.ts';
import { loadEpk } from '#lib/server/epk-page.ts';
import type { RequestHandler } from './$types';

/**
 * A press kit photo. Public ones come from the owner's repo for anyone; the rest come from the
 * press kit's space, only for the owner and people with access.
 */
export const GET: RequestHandler = async ({ locals, params, url }) => {
	const { app, viewer } = locals;
	const { epk } = await loadEpk(app.epks, params.did, params.rkey);
	const download = url.searchParams.has('download') ? 'attachment' : 'inline';
	const isPublic = epk.record.photos?.some((photo) => photo.image.ref.$link === params.cid);

	if (isPublic && app.core.fetchBlob) {
		const { bytes, mimeType } = await app.core.fetchBlob(epk.did, params.cid, EPK_PHOTO_MAX_BYTES);

		return respond(bytes, mimeType, 'public, max-age=86400, immutable', download);
	}

	const access = await app.epks.accessState(epk, viewer);

	if (access.kind !== 'owner' && access.kind !== 'granted') error(404, 'Photo not found');
	const content = await app.epks.content(epk);

	if (!content?.photos?.some((entry) => entry.image.ref.$link === params.cid) || !app.core.fetchSpaceBlob)
		error(404, 'Photo not found');
	const { bytes, mimeType } = await app.core.fetchSpaceBlob(epk.space, epk.did, params.cid);

	return respond(bytes, mimeType, 'private, max-age=3600', download);
};

function respond(bytes: Uint8Array, mimeType: string, cacheControl: string, disposition: string) {
	if (!mimeType.startsWith('image/')) error(404, 'Photo not found');

	return new Response(new Blob([new Uint8Array(bytes)], { type: mimeType }), {
		headers: {
			'content-type': mimeType,
			'cache-control': cacheControl,
			'content-disposition': disposition,
			'x-content-type-options': 'nosniff'
		}
	});
}
