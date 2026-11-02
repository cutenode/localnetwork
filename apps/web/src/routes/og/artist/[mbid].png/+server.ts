import { error } from '@sveltejs/kit';
import ArtistCard from '#lib/og/cards/ArtistCard.svelte';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { cardPaths } from '#lib/server/og/paths.ts';
import { renderCard } from '#lib/server/og/render.ts';
import { cards } from '#lib/server/og/serve.ts';
import { bareMbid } from '#lib/supports.ts';
import type { RequestHandler } from './$types';

/** How long an artist's card is kept, in seconds: its counts change as sets come in. */
const HOUR = 3600;

const CACHE_CONTROL = `public, max-age=${HOUR}`;

/**
 * An artist's link-preview card, for an artist a set credits by MusicBrainz ID. A kept card is served without asking
 * the directory; while the directory is offline, a crawler is asked to come back.
 */
export const GET: RequestHandler = async ({ params, locals }) => {
	const { app, log } = locals;
	const mbid = bareMbid(params.mbid);

	if (!mbid) error(404, 'Not found');
	const path = cardPaths.artist(mbid);
	const kept = cards.kept(path, CACHE_CONTROL);

	if (kept) return kept;
	const supports = await unlessOffline(log, () => app.supports.artist({ mbid }));

	if (!supports) {
		return new Response("The DJ directory isn't answering right now.", {
			status: 503,
			headers: { 'retry-after': '60', 'cache-control': 'no-store' }
		});
	}

	const { artist, tracks } = supports;

	if (!artist) error(404, 'Not found');

	const top = tracks
		.map(({ track }) => ({ title: track.title, supports: track.supports }))
		.sort((a, b) => b.supports - a.supports)
		.slice(0, 3);

	return cards.respond(
		log,
		path,
		() => renderCard(ArtistCard, { name: artist.name, djCount: artist.djCount, tracks: top }),
		{ cacheControl: CACHE_CONTROL, ttl: HOUR * 1000 }
	);
};
