import { error, redirect } from '@sveltejs/kit';
import { cacheBriefly } from '#lib/server/http.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { artistHref, bareMbid } from '#lib/supports.ts';
import type { PageServerLoad } from './$types';

/** An artist by MusicBrainz ID. Artists without one have their page under /artists/name/. */
export const load: PageServerLoad = (event) => {
	const { params, locals } = event;
	const mbid = bareMbid(params.id);

	if (!mbid) error(404, 'No artist with that ID');
	const canonical = artistHref({ name: '', mbid });

	if (event.url.pathname !== canonical) redirect(308, canonical);
	cacheBriefly(event);

	// Streamed, so the page shows while the directory counts. Null when it's offline.
	return {
		name: null,
		canonical,
		origin: locals.app.config.publicUrl,
		supports: unlessOffline(locals.log, () => locals.app.supports.artist({ mbid }))
	};
};
