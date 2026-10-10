import { HappyViewError } from '#lib/server/happyview/client.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const { projects, geocoder } = locals.app;

	let directory;

	try {
		directory = await Promise.all([projects.recent(6), projects.stats()]);
	} catch (error) {
		if (!(error instanceof HappyViewError)) throw error;

		// The home page still works without the directory: search and listing don't need it until they're used.
		return { recent: [], stats: null, genres: [], cities: [], geocoder: geocoder.enabled };
	}

	const [recent, stats] = directory;

	return {
		recent,
		stats: { active: stats.active, cities: stats.cityCount, unclaimed: stats.unclaimed },
		genres: stats.genres.slice(0, 14),
		cities: stats.cities.slice(0, 8),
		geocoder: geocoder.enabled
	};
};
