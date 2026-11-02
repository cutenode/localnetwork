import { cacheBriefly } from '#lib/server/http.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { pageMeta } from '#lib/server/meta.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = (event) => {
	const { url, locals } = event;
	const q = url.searchParams.get('q')?.trim().slice(0, 100) ?? '';
	cacheBriefly(event);

	// Streamed, so the search box shows while the directory looks. Null when it's offline.
	return {
		q,
		artists: unlessOffline(locals.log, () => locals.app.supports.search(q)),
		meta: pageMeta(locals.app.config.publicUrl, {
			title: 'Artists',
			description: "Find out which Local Network DJs played an artist's tracks, and where.",
			path: '/artists'
		})
	};
};
