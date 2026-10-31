import { error } from '@sveltejs/kit';
import { loadEditor } from '#lib/server/epk-page.ts';
import { imageResponse, PHOTO_TYPES } from '#lib/server/images.ts';
import type { RequestHandler } from './$types';

/** A photo in the editor: from the viewer's draft, or from the published press kit, public or not. Managers only. */
export const GET: RequestHandler = async ({ locals, url, params }) => {
	const { project, draft, epk } = await loadEditor(locals, url, params);
	const { epks } = locals.app;

	const photo =
		(draft && (await epks.drafts.photo(project, draft, params.cid))) ??
		(epk && (await epks.photo(epk, params.cid, true)));

	if (!photo) error(404, 'Photo not found');

	return imageResponse(new Uint8Array(photo.bytes), photo.mimeType, PHOTO_TYPES, {
		cacheControl: 'private, max-age=3600',
		download: url.searchParams.has('download'),
		log: locals.log
	});
};
