import type { Cookies } from '@sveltejs/kit';
import { z } from 'zod';
import { isValidLatLng } from '#lib/geo.ts';
import { parseJson } from './db';

/** The city picked on /shows last, so the page opens on it next time. */
const PLACE_COOKIE = 'ln_shows_place';

export const RADII = [10, 25, 50, 100, 250] as const;

const savedPlace = z.object({ place: z.string().max(200), lat: z.number(), lng: z.number(), radiusKm: z.number() });

export type ShowsPlace = z.infer<typeof savedPlace>;

/**
 * The place /shows lists shows near: the one in the URL, remembered for next time, or else the remembered one. A place
 * cleared in the form (sent without coordinates) is forgotten. A form without a radius (the Following tab's) keeps
 * the remembered one.
 */
export function showsPlace(url: URL, cookies: Pick<Cookies, 'get' | 'set' | 'delete'>, secure: boolean) {
	const params = url.searchParams;
	const saved = savedPlace.safeParse(parseJson<unknown>(cookies.get(PLACE_COOKIE), null)).data ?? null;

	if (!params.has('place')) return saved;
	const lat = Number(params.get('lat'));
	const lng = Number(params.get('lng'));

	if (!params.get('lat') || !isValidLatLng(lat, lng)) {
		cookies.delete(PLACE_COOKIE, { path: '/shows' });

		return null;
	}

	const radius = Number(params.get('radius'));

	const place: ShowsPlace = {
		place: params.get('place')?.slice(0, 200) ?? '',
		lat,
		lng,
		radiusKm: RADII.find((km) => km === radius) ?? saved?.radiusKm ?? 50
	};

	cookies.set(PLACE_COOKIE, JSON.stringify(place), {
		path: '/shows',
		httpOnly: true,
		sameSite: 'lax',
		secure,
		maxAge: 365 * 86_400
	});

	return place;
}
