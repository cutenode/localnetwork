import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { RADII, showsPlace } from '#lib/server/shows-place.ts';
import type { ShowView } from '#lib/types.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ url, cookies, locals }) => {
	const { app, viewer, log } = locals;
	const following = url.searchParams.get('view') === 'following';
	const place = showsPlace(url, cookies, app.config.publicUrl.startsWith('https:'));
	const near = place ? { lat: place.lat, lng: place.lng } : undefined;
	const cursor = url.searchParams.get('cursor') ?? undefined;

	// Streamed, so the page and its form show while the directory looks the shows up.
	const shows: Promise<{ items: ShowView[]; cursor?: string } | null> =
		following && !viewer
			? Promise.resolve({ items: [] })
			: unlessOffline(log, () =>
					following
						? app.shows.upcoming({ follower: viewer?.did, near, sort: near ? 'distance' : 'date', cursor })
						: app.shows.upcoming({ near, radiusKm: place?.radiusKm, cursor })
				);

	return { following, place, radii: RADII, geocoder: app.geocoder.enabled, shows };
};
