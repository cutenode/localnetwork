import { expect, it } from 'vitest';
import type { Place } from '#lib/places.ts';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

const PLACES: Place[] = [
	{ label: 'Berlin, Germany', city: 'Berlin', country: 'DE', latE6: 52_520_008, lngE6: 13_404_954 },
	{ label: 'Bergen, Norway', city: 'Bergen', region: 'Vestland', country: 'NO', latE6: 60_391_263, lngE6: 5_322_054 }
];

/** A geocoder over a fixed list of cities, by name prefix, allowing `searches` a burst per address. */
const app = (searches = 60) => ({
	limits: { search: new RateLimiter(searches, 2) },
	geocoder: {
		search: async (query: string) =>
			PLACES.filter((place) => query && place.city.toLowerCase().startsWith(query.toLowerCase()))
	}
});

it('finds cities starting with the query, cached privately', async () => {
	const response = await callRoute(GET, { app: app(), url: '/api/places?q=berl' });

	expect(response.headers.get('cache-control')).toBe('private, max-age=300');
	expect(await response.json()).toEqual({ places: [PLACES[0]] });
});

it('finds nothing without a query', async () => {
	const response = await callRoute(GET, { app: app(), url: '/api/places' });

	expect(await response.json()).toEqual({ places: [] });
});

it('finds nothing, with a 429, once an address searches too often', async () => {
	const response = await callRoute(GET, { app: app(0), url: '/api/places?q=berl' });

	expect(response.status).toBe(429);
	expect(await response.json()).toEqual({ places: [] });
});
