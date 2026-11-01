import { json } from '@sveltejs/kit';
import { sql } from 'kysely';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ locals }) => {
	const [, happyView] = await Promise.all([sql`SELECT 1`.execute(locals.app.db), locals.app.happyView.health()]);

	const backups = locals.app.backupStatus;

	// Still 200 when HappyView is down or the backups are stale: the app is up, and pages that need projects say the
	// directory is offline. `backup` is `stale` with none in the last two days, `off` without a bucket.
	return json(
		{
			ok: true,
			root: locals.app.rootDid,
			happyView: happyView ? 'ok' : 'unreachable',
			backup: !backups ? 'off' : backups.stale ? 'stale' : 'ok'
		},
		{ headers: { 'cache-control': 'no-store' } }
	);
};
