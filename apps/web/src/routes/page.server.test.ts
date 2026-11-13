import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import type { ProfileService } from '#lib/server/services/profiles.ts';
import { ProjectService } from '#lib/server/services/projects.ts';
import { ShowService } from '#lib/server/services/shows.ts';
import { ADMIN, DJ, FRIEND, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { loadRoute, routeEvent } from '#lib/testing/route-event.ts';
import type { Location, ProjectView, StrongRef } from '#lib/types.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The home page for booking DJs, while the socialHome flag is off. */
type Classic = Extract<PageServerData, { social: false }>;

/** The app as the home page reads it, with `projects`, and any of `services`, in place of the directory's own. */
const app = (
	projects: ProjectService = f.t.projects,
	services: { shows?: ShowService; profiles?: ProfileService } = {}
) => ({
	...f.t,
	projects,
	...services,
	geocoder: { enabled: true },
	config: { publicUrl: 'https://localnetwork.computer' }
});

it('shows the newest projects and the directory totals, with the tool cards and no captures', async () => {
	const data = await loadRoute<Classic>(load, { app: app(), url: '/' });

	expect(data.recent.map((project) => project.slug)).toEqual([f.project.slug]);
	expect(data.stats).toEqual({ active: 1, cities: 1, unclaimed: 0 });
	expect(data.genres).toEqual([expect.objectContaining({ value: 'techno', count: 1 })]);
	expect(data.cities).toHaveLength(1);
	expect(data.geocoder).toBe(true);
	expect(data.tools.map((tool) => tool.id)).toEqual(['locallibrary', 'localtoolbox']);
	expect(Object.keys(data.tools[0]).sort()).toEqual(['id', 'kind', 'lines', 'name', 'tagline']);
	expect(data.meta).toMatchObject({ url: 'https://localnetwork.computer/' });
	expect(await data.shows).toEqual([]);
	expect(await data.tracks).toEqual([]);
});

it('keeps only the top 14 genres and 8 cities', async () => {
	const admin = await f.t.viewer(ADMIN);

	for (let n = 0; n < 16; n++) {
		await f.t.projectAdmin.create({
			actor: admin,
			profile: { name: `DJ ${n}`, kind: 'solo', genres: [`genre-${n}`], location: { city: `City ${n}`, country: 'DE' } }
		});
	}

	const data = await loadRoute<Classic>(load, { app: app(), url: '/' });

	expect(data.genres).toHaveLength(14);
	expect(data.cities).toHaveLength(8);
});

it('still opens without the directory while HappyView is down', async () => {
	const offline = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response('Service Unavailable', { status: 503 })
	});

	const projects = new ProjectService(f.t.core, offline);
	const data = await loadRoute<Classic>(load, { app: app(projects), url: '/' });

	expect(data).toMatchObject({ recent: [], stats: null, genres: [], cities: [] });
	expect(data.tools).toHaveLength(2);
});

it('fails the page on any other error', async () => {
	// SAFETY: the object inherits every member of the real service; only `recent` is its own.
	const broken = Object.assign(Object.create(f.t.projects) as ProjectService, {
		recent: async () => {
			throw new TypeError('broken');
		}
	});

	await expect(loadRoute<Classic>(load, { app: app(broken), url: '/' })).rejects.toThrow('broken');
});

describe('with the social home on', () => {
	/** Next year, so the shows stay upcoming. */
	const YEAR = new Date().getUTCFullYear() + 1;

	const BERLIN = { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 } satisfies Location;

	const HAMBURG = { city: 'Hamburg', country: 'DE', latE6: 53_551_000, lngE6: 9_994_000 } satisfies Location;

	const PICKED = { place: 'Berlin', lat: 52.52, lng: 13.405, radiusKm: 50 };

	const remembered = { ln_shows_place: JSON.stringify(PICKED) };

	type Social = Extract<PageServerData, { social: true }>;

	const home = async (init: Parameters<typeof loadRoute>[1] = {}) => {
		const data = await loadRoute<PageServerData>(load, { app: app(), url: '/', flags: { socialHome: true }, ...init });

		if (!data.social) throw new Error('expected the social home');

		return data;
	};

	const uris = async (shows: Promise<{ uri: string }[] | null> | null) =>
		(await shows)?.map((show) => show.uri) ?? null;

	/** A Berlin act's show, and a Hamburg act's, both by DJ. */
	let neon: StrongRef;

	let hamburg: StrongRef;

	let berlinAct: ProjectView;

	beforeEach(async () => {
		const admin = await f.t.viewer(ADMIN);
		const dj = await f.t.viewer(DJ);

		const act = async (name: string, location: Location) =>
			f.t.projectAdmin.setManagers({
				actor: admin,
				project: await f.t.projectAdmin.create({
					actor: admin,
					profile: { name, kind: 'solo', genres: ['house'], location }
				}),
				managers: [DJ]
			});

		const show = (name: string, location: typeof BERLIN) => ({
			name,
			date: `${YEAR}-11-07`,
			start: '23:00',
			end: '06:00',
			venue: 'Halle Nord',
			location,
			cancelled: false
		});

		berlinAct = await act('Tidal Hours', BERLIN);
		neon = await f.t.shows.save(dj, berlinAct, show('Tidal Nights', BERLIN));
		hamburg = await f.t.shows.save(dj, await act('Harbour Lights', HAMBURG), show('Harbour Nights', HAMBURG));
	});

	it('asks a first visitor to pick a place, with the charts', async () => {
		const data = await home();

		expect(data).toMatchObject({ social: true, picked: null, network: null, geocoder: true });
		expect(await data.area).toBeNull();
		expect(await data.tracks).toEqual([]);
		expect(data.meta).toMatchObject({ url: 'https://localnetwork.computer/' });
	});

	it('lists the shows and DJs near the place picked last', async () => {
		const data = await home({ cookies: remembered });
		const area = await data.area;

		expect(data.picked).toEqual(PICKED);
		expect(area?.place).toEqual(PICKED);
		expect(area?.shows?.map((show) => show.uri)).toEqual([neon.uri]);
		expect(area?.djs?.map(({ project }) => project.slug)).toEqual([berlinAct.slug]);
	});

	it('remembers a place picked here for /shows too', async () => {
		const { loadData, cookies } = routeEvent({
			app: app(),
			url: '/?place=Hamburg&lat=53.551&lng=9.994',
			flags: { socialHome: true }
		});

		const data = await loadData<Social>(load);

		expect((await data.area)?.shows?.map((show) => show.uri)).toEqual([hamburg.uri]);
		expect(cookies.options.get('ln_shows_place')).toMatchObject({ path: '/' });
	});

	it("says what's offline while HappyView is down", async () => {
		const offline = new HappyViewClient({
			url: 'http://happyview.test',
			fetch: async () => new Response('', { status: 503 })
		});

		const projects = new ProjectService(f.t.core, offline);
		const shows = new ShowService(f.t.core, projects, offline, f.t.indexedRepo, 0);
		const data = await home({ app: app(projects, { shows }), cookies: remembered });

		expect(await data.area).toMatchObject({ place: PICKED, shows: null, djs: null });
	});

	it("leads a viewer with their network's shows, then what's near their home city without those", async () => {
		const fan = await f.t.viewer(FRIEND);

		await f.t.follows.follow(fan, berlinAct);
		await f.t.profiles.save(fan, { basics: { name: 'Fan', location: BERLIN }, avatar: 'keep', swap: null });

		const data = await home({
			viewer: fan,
			cookies: { ln_shows_place: JSON.stringify({ ...PICKED, place: 'Hamburg', lat: 53.551, lng: 9.994 }) }
		});

		const area = await data.area;

		expect(await uris(data.network)).toEqual([neon.uri]);
		expect(area?.place).toEqual({ place: 'Berlin', lat: 52.52, lng: 13.405, radiusKm: 50 });
		expect(area?.shows).toEqual([]);
	});

	it('leaves the DJs a viewer follows out of the DJs near them', async () => {
		const fan = await f.t.viewer(FRIEND);
		const admin = await f.t.viewer(ADMIN);

		const other = await f.t.projectAdmin.create({
			actor: admin,
			profile: { name: 'Saffron Social', kind: 'duo', genres: ['disco'], location: BERLIN }
		});

		await f.t.follows.follow(fan, berlinAct);

		const signedIn = await (await home({ viewer: fan, cookies: remembered })).area;
		const signedOut = await (await home({ cookies: remembered })).area;

		expect(signedIn?.djs?.map(({ project }) => project.slug)).toEqual([other.slug]);
		expect(signedOut?.djs?.map(({ project }) => project.slug).sort()).toEqual([berlinAct.slug, other.slug].sort());
	});

	it('uses the place picked last for a viewer with no home city, and nothing without one', async () => {
		const fan = await f.t.viewer(FRIEND);
		const picked = await home({ viewer: fan, cookies: remembered });
		const none = await home({ viewer: fan });

		expect(await uris(picked.network)).toEqual([]);
		expect((await picked.area)?.place).toEqual(PICKED);
		expect(await none.area).toBeNull();
	});

	it("falls back to the place picked when the viewer's profile can't be read", async () => {
		const fan = await f.t.viewer(FRIEND);
		const warned: string[] = [];

		// SAFETY: the object inherits every member of the real service; only `get` is its own.
		const profiles = Object.assign(Object.create(f.t.profiles) as ProfileService, {
			get: async () => {
				throw new Error('down');
			}
		});

		const log = { ...silentLogger, warn: (message: string) => void warned.push(message) };
		const data = await home({ app: app(f.t.projects, { profiles }), viewer: fan, cookies: remembered, log });

		expect((await data.area)?.place).toEqual(PICKED);
		expect(warned).toEqual(["couldn't read the viewer's home city"]);
	});
});
