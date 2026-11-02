import { cacheBriefly } from '#lib/server/http.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { pageMeta } from '#lib/server/meta.ts';
import { chartFilters, recentMonths } from '#lib/supports.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = (event) => {
	const { url, locals } = event;
	const today = new Date();
	const filters = chartFilters(url.searchParams, today);
	cacheBriefly(event);

	// Streamed, so the filters show while the directory counts. Null when it's offline.
	return {
		filters,
		months: recentMonths(today),
		charts: unlessOffline(locals.log, () => locals.app.supports.charts(filters)),
		meta: pageMeta(locals.app.config.publicUrl, {
			title: 'Charts',
			description: 'The tracks and artists Local Network DJs play most.',
			path: '/charts'
		})
	};
};
