import { afterEach, beforeEach, expect, it } from 'vitest';
import type { Place } from '#lib/places.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { PerformerSearch } from '#lib/server/services/performers.ts';
import { ADMIN, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();

	// Besides Neon Tide, a solo DJ from Berlin: a house collective from Lisbon.
	await f.t.projectAdmin.create({
		actor: await f.t.viewer(ADMIN),
		profile: { name: 'Kiln', kind: 'collective', genres: ['house'], location: { city: 'Lisbon', country: 'PT' } }
	});
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The app, with the site's public URL and no geocoder. */
const app = () => ({ ...f.t, config: { publicUrl: 'https://localnetwork.test' }, geocoder: { enabled: false } });

/** The names of the projects a search found. */
const found = (result: { items: { kind: string; project?: { profile: { name: string } } }[] } | null) =>
	result?.items.flatMap((item) => (item.project ? [item.project.profile.name] : []));

it('lists every DJ without filters, with the filter form empty', async () => {
	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/projects' });

	expect(found(data.result)?.toSorted()).toEqual(['Kiln', 'Neon Tide']);
	expect(data.result).toMatchObject({ page: 1, pageSize: 24 });
	expect(data.filters).toEqual({
		q: '',
		place: '',
		lat: '',
		lng: '',
		radiusKm: 50,
		genres: [],
		kinds: [],
		formats: [],
		setTypes: [],
		feeTiers: [],
		country: '',
		sort: '',
		unclaimed: false
	});
	expect(data.geocoder).toBe(false);
	expect(data.meta.url).toBe('https://localnetwork.test/projects');
});

it('finds DJs by name, by kind and by country', async () => {
	const byName = await loadRoute<PageServerData>(load, { app: app(), url: '/projects?q=neon' });
	const byKind = await loadRoute<PageServerData>(load, { app: app(), url: '/projects?kind=collective' });
	const byCountry = await loadRoute<PageServerData>(load, { app: app(), url: '/projects?country=pt' });

	expect(found(byName.result)).toEqual(['Neon Tide']);
	expect(found(byKind.result)).toEqual(['Kiln']);
	expect(found(byCountry.result)).toEqual(['Kiln']);
});

it('reads every filter from the address, leaving out values it does not know and capping the radius', async () => {
	const query = [
		'q=techno',
		'place=Berlin',
		'lat=52.52',
		'lng=13.405',
		'radius=9000',
		'genres=techno,%20house',
		'genres=techno',
		'kind=solo,bogus',
		'format=vinyl,tape',
		'setType=peak',
		'fee=emerging,priceless',
		'country=de',
		'sort=distance',
		'unclaimed',
		'page=2'
	].join('&');

	const data = await loadRoute<PageServerData>(load, { app: app(), url: `/projects?${query}` });

	expect(data.filters).toEqual({
		q: 'techno',
		place: 'Berlin',
		lat: '52.52',
		lng: '13.405',
		radiusKm: 2000,
		genres: ['techno', 'house'],
		kinds: ['solo'],
		formats: ['vinyl'],
		setTypes: ['peak'],
		feeTiers: ['emerging'],
		country: 'DE',
		sort: 'distance',
		unclaimed: true
	});
	expect(data.result).toMatchObject({ page: 2 });
});

it('ignores a place without valid coordinates, a country that is no ISO code, and an unknown sort', async () => {
	const outOfRange = await loadRoute<PageServerData>(load, {
		app: app(),
		url: '/projects?lat=91&lng=0&radius=1&country=DEU&sort=price'
	});

	const blank = await loadRoute<PageServerData>(load, { app: app(), url: '/projects?lat=&lng=13.4&sort=name' });

	expect(outOfRange.filters).toMatchObject({ lat: '', lng: '', radiusKm: 5, country: '', sort: '' });
	expect(blank.filters).toMatchObject({ lat: '', lng: '', sort: 'name' });
});

/** The app with a geocoder that knows `places`, recording what it was asked, under a limit of `limit` lookups. */
function geocoding(places: Record<string, Place[]>, limit = 10) {
	const asked: string[] = [];
	const geocoder = { enabled: true, search: async (query: string) => (asked.push(query), places[query] ?? []) };

	return { asked, app: { ...app(), geocoder, limits: { search: new RateLimiter(limit, 0) } } };
}

const berlin: Place = { label: 'Berlin, Germany', city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 };

it('looks a typed place up once, so the distance filter applies', async () => {
	const { asked, app: geocoded } = geocoding({ Berlin: [berlin] });

	const data = await loadRoute<PageServerData>(load, { app: geocoded, url: '/projects?place=Berlin&radius=25' });

	expect(asked).toEqual(['Berlin']);
	expect(data.filters).toMatchObject({ place: 'Berlin', lat: '52.52', lng: '13.405', radiusKm: 25 });
	expect(data.placeMissed).toBe(false);
	// Lisbon is far outside 25 km of Berlin.
	expect(found(data.result)).not.toContain('Kiln');
});

it("says when a typed place can't be found, and filters by nothing but the rest", async () => {
	const { app: geocoded } = geocoding({});

	const data = await loadRoute<PageServerData>(load, { app: geocoded, url: '/projects?place=Atlantis' });

	expect(data.placeMissed).toBe(true);
	expect(data.filters.lat).toBe('');
	expect(found(data.result)?.sort()).toEqual(['Kiln', 'Neon Tide']);
});

it('looks nothing up when the place came with coordinates, or past the limit', async () => {
	const { asked, app: geocoded } = geocoding({ Berlin: [berlin] }, 0);

	const picked = await loadRoute<PageServerData>(load, {
		app: geocoded,
		url: '/projects?place=Berlin&lat=52.52&lng=13.4'
	});

	const limited = await loadRoute<PageServerData>(load, { app: geocoded, url: '/projects?place=Berlin' });

	expect(asked).toEqual([]);
	expect(picked.placeMissed).toBe(false);
	expect(limited.placeMissed).toBe(true);
});

it('renders the filters with no results while the directory is offline', async () => {
	const directory = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response('Service Unavailable', { status: 503 })
	});

	const performers = new PerformerSearch(f.t.core, directory, f.t.projects, f.t.profiles);
	const data = await loadRoute<PageServerData>(load, { app: { ...app(), performers }, url: '/projects?q=neon' });

	expect(data.result).toBeNull();
	expect(data.filters.q).toBe('neon');
});
