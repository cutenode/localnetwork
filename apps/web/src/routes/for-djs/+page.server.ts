import { feeSummary } from '#lib/format.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { pageMeta } from '#lib/server/meta.ts';
import type { PageServerLoad } from './$types';

/** How many of the newest listings to look through for one with enough filled in to show. */
const CANDIDATES = 12;

export const load: PageServerLoad = ({ locals }) => {
	const { projects, config } = locals.app;

	// Streamed: a real listing as promoters see it, and the directory's totals. Null when it's offline.
	const directory = unlessOffline(locals.log, async () => {
		const [recent, stats] = await Promise.all([projects.recent(CANDIDATES), projects.stats()]);

		const isComplete = (project: (typeof recent)[number]) =>
			!!project.profile.tagline && !!feeSummary(project.profile) && project.profile.genres.length > 0;

		return {
			example: recent.find(isComplete) ?? recent.at(0) ?? null,
			djs: stats.active,
			cities: stats.cityCount,
			unclaimed: stats.unclaimed
		};
	});

	return {
		directory,
		meta: pageMeta(config.publicUrl, {
			title: 'For DJs',
			description: 'Get listed, or claim the listing an admin made, so promoters near you can find and book you.',
			path: '/for-djs'
		})
	};
};
