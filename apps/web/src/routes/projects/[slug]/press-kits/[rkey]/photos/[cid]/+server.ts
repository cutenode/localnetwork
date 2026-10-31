import { error } from '@sveltejs/kit';
import { imageResponse, PHOTO_TYPES } from '#lib/server/images.ts';
import type { RequestHandler } from './$types';

/**
 * A press kit photo. Public ones come from the root's repo for anyone; the rest come from the press kit's space, only
 * for the project's managers and people with access.
 */
export const GET: RequestHandler = async ({ locals, params, url }) => {
	const { app, viewer } = locals;
	const project = await app.projects.bySlug(params.slug);
	const isManager = !!viewer && !!project?.managers.includes(viewer.did);
	const hidden = !project || (project.status !== 'active' && !isManager && !viewer?.isStaff);
	const kit = hidden ? null : await app.epks.get(project, params.rkey);

	if (!kit) error(404, 'Photo not found');
	const access = viewer ? await app.epks.accessState(kit, viewer) : null;
	const photo = await app.epks.photo(kit.epk, params.cid, access?.kind === 'manager' || access?.kind === 'granted');

	if (!photo) error(404, 'Photo not found');

	return imageResponse(new Uint8Array(photo.bytes), photo.mimeType, PHOTO_TYPES, {
		cacheControl: photo.isPublic ? 'public, max-age=86400, immutable' : 'private, max-age=3600',
		download: url.searchParams.has('download'),
		log: locals.log
	});
};
