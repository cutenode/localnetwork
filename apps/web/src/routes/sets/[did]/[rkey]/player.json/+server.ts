import { error, json } from '@sveltejs/kit';
import { hideInactiveAccount } from '#lib/server/profile-page.ts';
import type { RequestHandler } from './$types';

/** What a set plays, asked for when someone presses Play on it (see RecordingService). */
export const GET: RequestHandler = async ({ params, locals }) => {
	const { app, log } = locals;
	await hideInactiveAccount(locals.app, params.did);
	const set = await app.recordings.set(params.did, params.rkey);

	if (!set) error(404, 'No set here');
	const source = await app.recordings.source(set, locals.flags);
	log.debug('set player source', {
		set: set.uri,
		source: source && (source.kind === 'audio' ? source.from : source.platform)
	});

	return json({ source }, { headers: { 'cache-control': 'public, max-age=60' } });
};
