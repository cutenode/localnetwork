import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { directorySample } from '#lib/server/landing.ts';
import { pageMeta } from '#lib/server/meta.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => {
	const { projects, config } = locals.app;

	// Streamed: a real listing as promoters see it, and the directory's totals. Null when it's offline.
	const directory = unlessOffline(locals.log, () => directorySample(projects));

	return {
		directory,
		meta: pageMeta(config.publicUrl, {
			title: 'For DJs',
			description: 'Get listed, or claim the listing an admin made, so promoters near you can find and book you.',
			path: '/for-djs'
		})
	};
};
