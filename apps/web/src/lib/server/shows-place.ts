import type { Cookies } from '@sveltejs/kit';
import { z } from 'zod';
import { isValidLatLng } from '#lib/geo.ts';
import { parseJson } from './db';

/** The city picked last, on /shows or the home page, so both open on it next time. */
const PLACE_COOKIE = 'ln_shows_place';

/** Where the cookie was kept before the home page read it too, deleted wherever it's written so it can't shadow it. */
const OLD_PATH = '/shows';

export const RADII = [10, 25, 50, 100, 250] as const;

const savedPlace = z.object({ place: z.string().max(200), lat: z.number(), lng: z.number(), radiusKm: z.number() });

export type ShowsPlace = z.infer<typeof savedPlace>;

/**
 * The place shows are listed near: the one in the URL, remembered for next time, or else the remembered one. A place
 * cleared in the form (sent without coordinates) is forgotten. A form without a radius (the Following tab's, the home
 * page's) keeps the remembered one.
 */
export function showsPlace(url: URL, cookies: Pick<Cookies, 'get' | 'set' | 'delete'>, secure: boolean) {
	const params = url.searchParams;
	const saved = savedPlace.safeParse(parseJson<unknown>(cookies.get(PLACE_COOKIE), null)).data ?? null;

	if (!params.has('place')) return saved;
	const lat = Number(params.get('lat'));
	const lng = Number(params.get('lng'));

	cookies.delete(PLACE_COOKIE, { path: OLD_PATH });

	if (!params.get('lat') || !isValidLatLng(lat, lng)) {
		cookies.delete(PLACE_COOKIE, { path: '/' });

		return null;
	}

	const radius = Number(params.get('radius'));

	const place: ShowsPlace = {
		// The URL has a place, checked above, so it's a string.
		place: String(params.get('place')).slice(0, 200),
		lat,
		lng,
		radiusKm: RADII.find((km) => km === radius) ?? saved?.radiusKm ?? 50
	};

	cookies.set(PLACE_COOKIE, JSON.stringify(place), {
		path: '/',
		httpOnly: true,
		sameSite: 'lax',
		secure,
		maxAge: 365 * 86_400
	});

	return place;
}
