import { HappyViewError } from '#lib/server/happyview/client.ts';
import { toolCards } from '#lib/tools/index.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const { projects, geocoder } = locals.app;

	// The tools' docs are bundled server-side, so the page gets only what its cards show.
	const page = { geocoder: geocoder.enabled, tools: toolCards() };

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
