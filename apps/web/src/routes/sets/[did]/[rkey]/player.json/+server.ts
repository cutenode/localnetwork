import { error, json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';

/** What a set plays, asked for when someone presses Play on it (see RecordingService). */
export const GET: RequestHandler = async ({ params, locals }) => {
	const { app, log } = locals;
	const set = await app.recordings.set(params.did, params.rkey);

	if (!set) error(404, 'No set here');
	const source = await app.recordings.source(set);
	log.debug(
		{ set: set.uri, source: source && (source.kind === 'audio' ? source.from : source.platform) },
		'set player source'
	);

	return json({ source }, { headers: { 'cache-control': 'public, max-age=60' } });
};
