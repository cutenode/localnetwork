import { redirect } from '@sveltejs/kit';
import { cacheBriefly } from '#lib/server/http.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { artistMeta } from '#lib/server/meta.ts';
import { artistHref } from '#lib/supports.ts';
import type { PageServerLoad } from './$types';

/** An artist by name, from the path, or the query for a name a path can't carry (see artistHref). */
export const load: PageServerLoad = async (event) => {
	const { params, url, locals } = event;
	const name = params.name ?? url.searchParams.get('name') ?? '';

	if (!name.trim()) redirect(307, '/artists');
	cacheBriefly(event);

	// Waited for, not streamed, so the head can name its canonical address: the MBID page, when sets give the name
	// one. Null when the directory is offline.
	const supports = await unlessOffline(locals.log, () => locals.app.supports.artist({ name }));
	const artist = supports?.artist ?? { name };

	return {
		supports,
		meta: artistMeta(locals.app.config.publicUrl, artist.name, artistHref(artist), !!supports?.artist)
	};
};
