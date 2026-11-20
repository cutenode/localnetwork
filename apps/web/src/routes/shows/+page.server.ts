import type { Listing } from '#lib/events.ts';
import { upcomingListings } from '#lib/server/events-list.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { pageMeta } from '#lib/server/meta.ts';
import { RADII, showsPlace } from '#lib/server/shows-place.ts';
import type { PageServerLoad } from './$types';

/**
 * The Events list: upcoming shows and, once party pages are on, parties' published events beside them, near a place
 * or everywhere. Following lists only shows: the directory works out which shows a viewer's follows reach, and
 * nothing does that for events yet.
 */
export const load: PageServerLoad = async ({ url, cookies, locals }) => {
	const { app, viewer, log, flags } = locals;
	const following = url.searchParams.get('view') === 'following';
	const place = showsPlace(url, cookies, app.config.publicUrl.startsWith('https:'));
	const near = place ? { lat: place.lat, lng: place.lng } : undefined;
	const cursor = url.searchParams.get('cursor') ?? undefined;

	// Streamed, so the page and its form show while the directory looks the shows up.
	const shows: Promise<{ items: Listing[]; cursor?: string } | null> =
		following && !viewer
			? Promise.resolve({ items: [] })
			: unlessOffline(log, () =>
					following
						? app.shows.upcoming({ follower: viewer?.did, near, sort: near ? 'distance' : 'date', cursor })
						: upcomingListings(app, { near, radiusKm: place?.radiusKm, cursor, events: flags.spacePages })
				);

	return {
		following,
		place,
		radii: RADII,
		geocoder: app.geocoder.enabled,
		shows,
		meta: pageMeta(app.config.publicUrl, {
			title: 'Shows',
			description: 'Upcoming shows by DJs on Local Network, near you or from the DJs you follow.',
			path: '/shows'
		})
	};
};
