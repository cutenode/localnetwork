import { error, redirect } from '@sveltejs/kit';
import { cacheBriefly } from '#lib/server/http.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { artistMeta } from '#lib/server/meta.ts';
import { musicBrainzArtist, withJsonLd } from '#lib/server/structured-data.ts';
import { artistHref, bareMbid } from '#lib/supports.ts';
import type { PageServerLoad } from './$types';

/** An artist by MusicBrainz ID. Artists without one have their page under /artists/name/. */
export const load: PageServerLoad = async (event) => {
	const { params, locals } = event;
	const mbid = bareMbid(params.id);

	if (!mbid) error(404, 'No artist with that ID');
	const canonical = artistHref({ name: '', mbid });

	if (event.url.pathname !== canonical) redirect(308, canonical);
	cacheBriefly(event);

	// Waited for, not streamed: the artist's name is only in what the directory counts, and link previews need it in
	// the head. Null when it's offline.
	const supports = await unlessOffline(locals.log, () => locals.app.supports.artist({ mbid }));

	return {
		supports,
		meta: withJsonLd(
			artistMeta(locals.app.config.publicUrl, supports?.artist?.name ?? null, canonical, !!supports?.artist),
			supports?.artist ? musicBrainzArtist(supports.artist.name, mbid) : null
		)
	};
};
