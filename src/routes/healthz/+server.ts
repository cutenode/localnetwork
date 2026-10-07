import { json } from '@sveltejs/kit';
import { sql } from 'kysely';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ locals }) => {
	const [, happyView] = await Promise.all([sql`SELECT 1`.execute(locals.app.db), locals.app.happyView.health()]);

	// Still 200 when HappyView is down: the app is up, and pages that need projects say the directory is offline.
	return json(
		{ ok: true, root: locals.app.rootDid, happyView: happyView ? 'ok' : 'unreachable' },
		{ headers: { 'cache-control': 'no-store' } }
	);
};
