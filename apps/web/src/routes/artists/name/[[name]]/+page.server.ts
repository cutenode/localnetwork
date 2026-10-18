import { redirect } from '@sveltejs/kit';
import { cacheBriefly } from '#lib/server/http.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import type { PageServerLoad } from './$types';

/** An artist by name, from the path, or the query for a name a path can't carry (see artistHref). */
export const load: PageServerLoad = (event) => {
	const { params, url, locals } = event;
	const name = params.name ?? url.searchParams.get('name') ?? '';

	if (!name.trim()) redirect(307, '/artists');
	cacheBriefly(event);

	// Streamed, so the page shows while the directory counts. Null when it's offline.
	return {
		name,
		canonical: null,
		origin: locals.app.config.publicUrl,
		supports: unlessOffline(locals.log, () => locals.app.supports.artist({ name }))
	};
};
