import { error } from '@sveltejs/kit';
import { requireViewer } from '#lib/server/http.ts';
import { AVATAR_TYPES, imageResponse } from '#lib/server/images.ts';
import type { RequestHandler } from './$types';

/** Streams a proposed avatar from the requester's PDS using the root's space credential, for them or staff. */
export const GET: RequestHandler = async ({ params, locals, url }) => {
	const viewer = requireViewer(locals, url);
	const { app } = locals;
	const request = await app.requests.byRkey(params.rkey);

	if (!request || (request.requester.did !== viewer.did && !viewer.isStaff)) error(404, 'Request not found');
	const avatar = request.record.profile?.avatar;

	if (!avatar || !app.core.fetchSpaceBlob) error(404, 'No image');

	let blob: { bytes: Uint8Array; mimeType: string };

	try {
		blob = await app.core.fetchSpaceBlob(app.spaces.moderation, request.repoDid, avatar.ref.$link);
	} catch (cause) {
		locals.log.warn({ err: cause }, 'could not fetch proposed avatar');
		error(502, "Could not fetch the image from the requester's PDS");
	}

	return imageResponse(new Uint8Array(blob.bytes), blob.mimeType, AVATAR_TYPES, {
		cacheControl: 'private, max-age=3600',
		log: locals.log
	});
};
