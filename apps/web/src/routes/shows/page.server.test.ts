import { afterEach, beforeEach, expect, it } from 'vitest';
import type { StrongRef } from '#lib/types.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { ShowService } from '#lib/server/services/shows.ts';
import { DJ, FRIEND, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { loadRoute, routeEvent } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

/** Next year, so the shows stay upcoming. */
const YEAR = new Date().getUTCFullYear() + 1;

const BERLIN = 'place=Berlin&lat=52.52&lng=13.405';

let f: Awaited<ReturnType<typeof pressKitServices>>;

/** Neon Tide's show in Berlin. */
let berlin: StrongRef;

/** Neon Tide's show in Hamburg, the night before. */
let hamburg: StrongRef;

beforeEach(async () => {
	f = await pressKitServices();
	const dj = await f.t.viewer(DJ);

	berlin = await f.t.shows.save(dj, f.project, {
		name: 'Tidal Hours',
		date: `${YEAR}-11-07`,
		start: '23:00',
		end: '06:00',
		venue: 'Halle Nord',
		location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 },
		cancelled: false
	});

	hamburg = await f.t.shows.save(dj, f.project, {
		name: 'Harbour Lights',
		date: `${YEAR}-11-06`,
		start: '23:00',
		end: '05:00',
		venue: 'Hafenklang',
		location: { city: 'Hamburg', country: 'DE', latE6: 53_551_000, lngE6: 9_994_000 },
		cancelled: false
	});
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The app, with the site's public URL and a geocoder. */
const app = () => ({ ...f.t, config: { publicUrl: 'https://localnetwork.test' }, geocoder: { enabled: true } });

/** The shows a page's stream lists, by URI, or null when the directory was offline. */
const listed = async (shows: Promise<{ items: { uri: string }[] } | null>) =>
	(await shows)?.items.map((item) => item.uri) ?? null;

it('lists every upcoming show without a place', async () => {
	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/shows' });

	expect(data).toMatchObject({ following: false, place: null, radii: [10, 25, 50, 100, 250], geocoder: true });
	expect(await listed(data.shows)).toEqual([hamburg.uri, berlin.uri]);
	expect(data.meta.url).toBe('https://localnetwork.test/shows');
});

it('lists the shows near the place in the address, and remembers it for next time', async () => {
	const { loadData, cookies } = routeEvent({ app: app(), url: `/shows?${BERLIN}&radius=25` });
	const data = await loadData<PageServerData>(load);
	const far = await loadRoute<PageServerData>(load, { app: app(), url: '/shows?place=Tokyo&lat=35.68&lng=139.69' });

	expect(data.place).toEqual({ place: 'Berlin', lat: 52.52, lng: 13.405, radiusKm: 25 });
	expect(await listed(data.shows)).toEqual([berlin.uri]);
	expect(cookies.options.get('ln_shows_place')).toMatchObject({ secure: true, path: '/shows' });
	expect(await listed(far.shows)).toEqual([]);
});

it('lists the shows of the DJs a viewer follows, nearest first once a place is remembered', async () => {
	const fan = await f.t.viewer(FRIEND);
	const before = await loadRoute<PageServerData>(load, { app: app(), viewer: fan, url: '/shows?view=following' });

	await f.t.follows.follow(fan, f.project);

	const after = await loadRoute<PageServerData>(load, { app: app(), viewer: fan, url: '/shows?view=following' });
	const place = JSON.stringify({ place: 'Berlin', lat: 52.52, lng: 13.405, radiusKm: 25 });

	const near = await loadRoute<PageServerData>(load, {
		app: app(),
		viewer: fan,
		url: '/shows?view=following',
		cookies: { ln_shows_place: place }
	});

	expect(before.following).toBe(true);
	expect(await listed(before.shows)).toEqual([]);
	expect(await listed(after.shows)).toEqual([hamburg.uri, berlin.uri]);
	expect(await listed(near.shows)).toEqual([berlin.uri, hamburg.uri]);
});

it("shows no one's shows on the Following tab while signed out, without asking the directory", async () => {
	const shows = new ShowService(f.t.core, f.t.projects, offlineDirectory(), f.t.indexedRepo, 0);
	const data = await loadRoute<PageServerData>(load, { app: { ...app(), shows }, url: '/shows?view=following' });

	expect(await data.shows).toEqual({ items: [] });
});

it('streams null for the shows while the directory is offline, so the form still shows', async () => {
	const shows = new ShowService(f.t.core, f.t.projects, offlineDirectory(), f.t.indexedRepo, 0);
	const data = await loadRoute<PageServerData>(load, { app: { ...app(), shows }, url: '/shows' });

	expect(await data.shows).toBeNull();
});

/** A directory that answers every query with a 503, as HappyView does while it's down. */
function offlineDirectory() {
	return new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response('Service Unavailable', { status: 503 })
	});
}
