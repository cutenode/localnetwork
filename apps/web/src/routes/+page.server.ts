import { HappyViewError } from '#lib/server/happyview/client.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { chartFilters } from '#lib/supports.ts';
import { pageMeta } from '#lib/server/meta.ts';
import { SITE_NAME } from '#lib/meta.ts';
import { toolCards } from '#lib/tools/index.ts';
import type { PageServerLoad } from './$types';

/** How many upcoming shows and most played tracks the home page lists. */
const SHOWS = 3;

const TRACKS = 5;

export const load: PageServerLoad = async ({ locals }) => {
	const { projects, shows, supports, geocoder, config } = locals.app;

	// The tools' docs are bundled server-side, so the page gets only what its cards show: no captures.
	const page = {
		geocoder: geocoder.enabled,
		// Streamed, so the page paints before the directory answers. Null when it's offline.
		shows: unlessOffline(locals.log, async () => (await shows.upcoming({ limit: SHOWS })).items),
		tracks: unlessOffline(locals.log, async () =>
			(await supports.charts(chartFilters(new URLSearchParams(), new Date()))).tracks.slice(0, TRACKS)
		),
		tools: toolCards().map(({ id, name, kind, tagline, lines }) => ({ id, name, kind, tagline, lines })),
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
