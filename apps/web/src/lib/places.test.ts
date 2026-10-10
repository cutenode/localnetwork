import { afterEach, describe, expect, it, vi } from 'vitest';
import { searchPlaces } from './places.ts';

const saoPaulo = {
	label: 'São Paulo, Brazil',
	city: 'São Paulo',
	region: 'São Paulo',
	country: 'BR',
	latE6: -23_550_520,
	lngE6: -46_633_309
};

/** Stands in for the server: `/api/places?q=São Paulo` answers with `body` and `status`, anything else is a 404. */
function serve(body: string, status = 200) {
	vi.stubGlobal('fetch', async (url: string) =>
		url === '/api/places?q=S%C3%A3o%20Paulo' ? new Response(body, { status }) : new Response(null, { status: 404 })
	);
}

afterEach(() => vi.unstubAllGlobals());

describe('searchPlaces', () => {
	it('returns the places the server found', async () => {
		serve(JSON.stringify({ places: [saoPaulo] }));
		expect(await searchPlaces('São Paulo')).toEqual([saoPaulo]);
	});

	it('finds nothing when the server refuses the search', async () => {
		serve('Too many requests', 429);
		expect(await searchPlaces('São Paulo')).toEqual([]);
	});
});
