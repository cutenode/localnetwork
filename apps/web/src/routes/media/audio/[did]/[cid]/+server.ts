import { isDidString } from '@atproto/oauth-scopes';
import { error } from '@sveltejs/kit';
import { recordingAudio } from '#lib/server/media/audio.ts';
import type { RequestHandler } from './$types';

const CID = /^[a-z2-7]{20,100}$/;

/** DJs' recordings on their own PDS, for the set player (see recordingAudio). */
export const GET: RequestHandler = async ({ params, request, locals, fetch }) => {
	const { app, log } = locals;

	if (!isDidString(params.did) || !CID.test(params.cid)) error(404, 'Not found');

	return recordingAudio({
		did: params.did,
		cid: params.cid,
		range: request.headers.get('range'),
		audio: (did, cid) => app.recordings.audio(did, cid),
		pds: async (did) => (await app.identity.resolve(did)).pds,
		fetch,
		log
	});
};
