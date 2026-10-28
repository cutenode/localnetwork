import { error } from '@sveltejs/kit';
import { loadEditor } from '#lib/server/epk-page.ts';
import type { RequestHandler } from './$types';

/** A photo in the editor: from the viewer's draft, or from the published press kit, public or not. Managers only. */
export const GET: RequestHandler = async ({ locals, url, params }) => {
	const { project, draft, epk } = await loadEditor(locals, url, params);
	const { epks } = locals.app;

	const photo =
		(draft && (await epks.drafts.photo(project, draft, params.cid))) ??
		(epk && (await epks.photo(epk, params.cid, true)));

	if (!photo?.mimeType.startsWith('image/')) error(404, 'Photo not found');

	return new Response(new Blob([new Uint8Array(photo.bytes)], { type: photo.mimeType }), {
		headers: {
			'content-type': photo.mimeType,
			'cache-control': 'private, max-age=3600',
			'x-content-type-options': 'nosniff'
		}
	});
};
