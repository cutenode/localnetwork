import { afterEach, beforeEach, expect, it } from 'vitest';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import type { Viewer } from '#lib/types.ts';
import type { Place } from '#lib/places.ts';
import type { VenuesResponse } from '#lib/venues.ts';
import { GET, POST } from './+server.ts';

const ADA = 'did:plc:ada';

const berlin = { city: 'Berlin', country: 'de', latE6: 52_520_000, lngE6: 13_405_000 };

let t: Awaited<ReturnType<typeof createTestServices>>;

let ada: Viewer;

beforeEach(async () => {
	t = await createTestServices({ flagOverrides: { spacePages: true } });
	ada = await t.viewer(ADA);
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

const app = (searches = 60) => ({ venues: t.venues, limits: { search: new RateLimiter(searches, 2) } });

const add = (body: { name: string; city: Partial<Place> }, viewer: Viewer | null = ada) =>
	callRoute(POST, {
		app: app(),
		viewer,
		flags: { spacePages: true },
		request: new Request('https://localnetwork.test/api/venues', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(body)
		})
	});

const search = (q: string, searches?: number) =>
	callRoute(GET, { app: app(searches), url: `/api/venues?q=${encodeURIComponent(q)}`, flags: { spacePages: true } });

it('adds a venue by name and city with no place ID, and links a second add of the same to the first', async () => {
	const first = await add({ name: 'Kater Blau', city: berlin });

	expect(first.status).toBe(200);
	const { venue } = await first.json();
	expect(venue).toEqual({
		space: expect.stringContaining('computer.localnetwork.space.venue'),
		skey: expect.any(String),
		name: 'Kater Blau',
		locality: 'Berlin',
		country: 'DE'
	});
	expect(await t.venues.bySpace(venue.space)).toMatchObject({
		address: { country: 'DE', locality: 'Berlin' },
		geo: { latitude: '52.52', longitude: '13.405' }
	});
	expect((await t.venues.bySpace(venue.space))?.fsqPlaceId).toBeUndefined();

	expect(venue.space).toContain(venue.skey);

	const again = await add({ name: ' kater  blau ', city: berlin });
	expect(await again.json()).toEqual({ venue });
	expect(await t.db.selectFrom('venue').select('name').execute()).toEqual([{ name: 'Kater Blau' }]);

	// The same name somewhere else is another venue.
	const hamburg = await add({ name: 'Kater Blau', city: { ...berlin, city: 'Hamburg', region: 'Hamburg' } });
	const other = (await hamburg.json()).venue;
	expect(other).toMatchObject({ name: 'Kater Blau', locality: 'Hamburg' });
	expect(other.space).not.toBe(venue.space);
	expect((await t.venues.bySpace(other.space))?.address).toEqual({
		country: 'DE',
		locality: 'Hamburg',
		region: 'Hamburg'
	});
});

it('finds venues by any part of their name, shortest first, and nothing for less than two letters', async () => {
	for (const name of ['Tresor.West', 'Tresor', 'Berghain']) await add({ name, city: berlin });
	await add({ name: '100% Club', city: berlin });

	const found = async (q: string) => {
		// SAFETY: the route answers a search with a VenuesResponse.
		const { venues } = (await (await search(q)).json()) as VenuesResponse;

		return venues.map((venue) => venue.name);
	};

	expect(await found('tres')).toEqual(['Tresor', 'Tresor.West']);
	expect(await found('HAIN')).toEqual(['Berghain']);
	// A wildcard in the query is only itself.
	expect(await found('0%')).toEqual(['100% Club']);
	expect(await found('r%')).toEqual([]);
	expect(await found('_e')).toEqual([]);
	expect(await found('t')).toEqual([]);

	const noQuery = await callRoute(GET, { app: app(), url: '/api/venues', flags: { spacePages: true } });
	expect(await noQuery.json()).toEqual({ venues: [] });
});

it('finds nothing, with a 429, once an address searches too often', async () => {
	const response = await search('tres', 0);

	expect(response.status).toBe(429);
	expect(await response.json()).toEqual({ venues: [] });
});

it('asks signed-out people to sign in, and refuses an add without a city from the search or with a long name', async () => {
	const signedOut = await add({ name: 'Kater Blau', city: berlin }, null);
	expect(signedOut.status).toBe(401);
	expect(await signedOut.json()).toEqual({ error: 'Sign in to add a venue' });

	const noCity = await add({ name: 'Kater Blau', city: { city: 'Berlin' } });
	expect(noCity.status).toBe(400);
	expect(await noCity.json()).toEqual({ error: 'Pick the city from the search' });

	const long = await add({ name: 'x'.repeat(65), city: berlin });
	expect(long.status).toBe(400);
	expect(await long.json()).toEqual({ error: 'The name can be at most 64 characters' });

	const blank = await add({ name: '  ', city: berlin });
	expect(await blank.json()).toEqual({ error: "Enter the venue's name" });
	expect(await t.db.selectFrom('venue').selectAll().execute()).toEqual([]);
});

it("refuses a body that is not JSON, and lets errors that are not the visitor's through", async () => {
	const garbled = await callRoute(POST, {
		app: app(),
		viewer: ada,
		flags: { spacePages: true },
		request: new Request('https://localnetwork.test/api/venues', { method: 'POST', body: '{' })
	});

	expect(garbled.status).toBe(400);

	const down = { ...app(), venues: { add: () => Promise.reject(new Error('the PDS is down')) } };

	const request = new Request('https://localnetwork.test/api/venues', {
		method: 'POST',
		body: JSON.stringify({ name: 'Kater Blau', city: berlin })
	});

	await expect(callRoute(POST, { app: down, viewer: ada, flags: { spacePages: true }, request })).rejects.toThrow(
		'the PDS is down'
	);
});

it("is a 404 while spacePages is off for the viewer, since events aren't out yet", async () => {
	await expect(callRoute(GET, { app: app(), url: '/api/venues?q=tres' })).rejects.toMatchObject({ status: 404 });
	await expect(callRoute(POST, { app: app(), viewer: ada })).rejects.toMatchObject({ status: 404 });
});
