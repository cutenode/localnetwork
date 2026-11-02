import { HappyViewError } from '#lib/server/happyview/client.ts';
import { pageMeta } from '#lib/server/meta.ts';
import { SITE_NAME } from '#lib/meta.ts';
import { toolCards } from '#lib/tools/index.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const { projects, geocoder, config } = locals.app;

	// The tools' docs are bundled server-side, so the page gets only what its cards show.
	const page = {
		geocoder: geocoder.enabled,
		tools: toolCards(),
		meta: pageMeta(config.publicUrl, {
			title: SITE_NAME,
			documentTitle: `${SITE_NAME} · Find local DJs for your show`,
			description:
				'Promoters find local DJs who fit their show. DJs get a profile they control, built on the AT Protocol.',
			path: '/'
		})
	};

	let directory;

	try {
		directory = await Promise.all([projects.recent(6), projects.stats()]);
	} catch (error) {
		if (!(error instanceof HappyViewError)) throw error;

		// The home page still works without the directory: search and listing don't need it until they're used.
		return { ...page, recent: [], stats: null, genres: [], cities: [] };
	}

	const [recent, stats] = directory;

	return {
		...page,
		recent,
		stats: { active: stats.active, cities: stats.cityCount, unclaimed: stats.unclaimed },
		genres: stats.genres.slice(0, 14),
		cities: stats.cities.slice(0, 8)
	};
};
