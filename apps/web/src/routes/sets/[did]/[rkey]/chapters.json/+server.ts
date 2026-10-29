import { error } from '@sveltejs/kit';
import { setChapters } from '#lib/mix/chapters.ts';
import type { RequestHandler } from './$types';

/** A set's tracklist as Podcasting 2.0 JSON chapters, for podcast apps playing its recording. */
export const GET: RequestHandler = async ({ params, locals }) => {
	const set = await locals.app.recordings.set(params.did, params.rkey);

	if (!set) error(404, 'No set here');

	// The Podcasting 2.0 JSON chapters format's media type.
	return new Response(JSON.stringify(setChapters(set)), {
		headers: { 'content-type': 'application/json+chapters', 'cache-control': 'public, max-age=300' }
	});
};
