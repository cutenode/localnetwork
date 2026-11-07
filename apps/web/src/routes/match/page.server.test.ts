import { afterEach, beforeEach, expect, it } from 'vitest';
import { HappyViewError } from '#lib/server/happyview/client.ts';
import { NO_SPACES, pressKitServices, PROMOTER } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The app on its public address, with the geocoder on. */
const app = () => ({ ...f.t, config: { publicUrl: 'https://localnetwork.test' }, geocoder: { enabled: true } });

it('opens an empty form without searching', async () => {
	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/match' });

	expect(data).toMatchObject({
		searched: false,
		results: [],
		geocoder: true,
		criteria: {
			title: '',
			date: '',
			place: '',
			lat: '',
			lng: '',
			location: undefined,
			radiusKm: 50,
			genres: [],
			budgetTier: undefined,
			capacity: undefined,
			setType: undefined,
			formats: []
		},
		meta: { title: 'Match a show', url: 'https://localnetwork.test/match' }
	});
});

it('reads the show from the query, keeping only values the form offers', async () => {
	const query = new URLSearchParams([
		['title', 'Rooftop Rave'],
		['date', '2026-12-12T23:30'],
		['place', 'The Lot, Berlin'],
		['lat', '52.52'],
		['lng', '13.405'],
		['radius', '5000'],
		['genres', 'techno,electro'],
		['budget', 'emerging'],
		['capacity', '300'],
		['setType', 'peak'],
		['format', 'vinyl'],
		['format', 'vinyl'],
		['format', 'wax-cylinder']
	]);

	const data = await loadRoute<PageServerData>(load, { app: app(), url: `/match?${query}` });

	expect(data).toMatchObject({
		criteria: {
			title: 'Rooftop Rave',
			date: '2026-12-12',
			place: 'The Lot, Berlin',
			lat: '52.52',
			lng: '13.405',
			location: { lat: 52.52, lng: 13.405 },
			radiusKm: 2000,
			genres: ['techno', 'electro'],
			budgetTier: 'emerging',
			capacity: 300,
			setType: 'peak',
			formats: ['vinyl']
		}
	});
});

it('drops a location off the map, and values the form never offers', async () => {
	const data = await loadRoute<PageServerData>(load, {
		app: app(),
		url: '/match?lat=123&lng=13&radius=1&budget=free-beer&capacity=12.5&setType=marathon'
	});

	expect(data).toMatchObject({
		criteria: {
			lat: '',
			lng: '',
			location: undefined,
			radiusKm: 5,
			budgetTier: undefined,
			capacity: undefined,
			setType: undefined
		}
	});
});

it('lists the DJs that fit the show', async () => {
	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/match?genres=techno&go=1' });

	expect(data).toMatchObject({ searched: true, results: [{ candidate: { profile: { name: 'Neon Tide' } } }] });
});

it('keeps the form when the directory is offline', async () => {
	const offline = {
		...app(),
		projects: {
			match: async () => {
				throw new HappyViewError(503, 'HappyView is down');
			}
		}
	};

	const data = await loadRoute<PageServerData>(load, { app: offline, url: '/match?title=Rooftop+Rave&go=1' });

	expect(data).toMatchObject({ searched: true, results: null, criteria: { title: 'Rooftop Rave' } });
});

it('saves the show and its shortlist to the planner', async () => {
	const promoter = await f.t.viewer(PROMOTER);

	const query =
		'title=Rooftop+Rave&date=2026-12-12&place=The+Lot&capacity=300&budget=emerging&setType=peak&genres=techno';

	await expect(
		callRoute(actions.saveShow, {
			app: app(),
			viewer: promoter,
			url: `/match?/saveShow&${query}`,
			form: { project: [f.project.uri, ' '] }
		})
	).rejects.toMatchObject({ status: 303, location: expect.stringMatching(/^\/dashboard\/planner\/shows\/\w+$/) });

	expect(await f.t.planner.shows(promoter)).toMatchObject([
		{
			details: {
				title: 'Rooftop Rave',
				date: expect.stringMatching(/^2026-12-12/),
				venue: 'The Lot',
				capacity: 300,
				budgetTier: 'emerging',
				setType: 'peak',
				genres: ['techno']
			},
			lineup: [{ project: f.project.uri, status: 'considering' }]
		}
	]);
});

it('names an untitled show by its date, or just "Show"', async () => {
	const promoter = await f.t.viewer(PROMOTER);

	for (const query of ['date=2026-12-12', ''])
		await expect(
			callRoute(actions.saveShow, { app: app(), viewer: promoter, url: `/match?/saveShow&${query}`, form: {} })
		).rejects.toMatchObject({ status: 303 });

	const titles = (await f.t.planner.shows(promoter)).map((show) => show.details.title);

	expect(titles.toSorted()).toEqual(['Show', 'Show on 2026-12-12']);
});

it("says why the show can't be saved for an account without spaces", async () => {
	const result = await callRoute(actions.saveShow, {
		app: app(),
		viewer: await f.t.viewer(NO_SPACES),
		url: '/match?/saveShow&title=Rooftop+Rave',
		form: { project: f.project.uri }
	});

	expect(result).toMatchObject({
		status: 400,
		data: { error: "Your PDS doesn't support atproto spaces yet, so the private planner isn't available" }
	});
});

it('sends someone signed out to sign in, back to the same search', async () => {
	await expect(
		callRoute(actions.saveShow, { app: app(), url: '/match?/saveShow&title=Rooftop', form: {} })
	).rejects.toMatchObject({ status: 303, location: '/login?next=%2Fmatch%3F%2FsaveShow%26title%3DRooftop' });
});
