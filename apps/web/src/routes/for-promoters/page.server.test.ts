import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { ProjectService } from '#lib/server/services/projects.ts';
import { ADMIN, pressKitServices, PROMOTER, STRANGER } from '#lib/server/testing/press-kits.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
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

const app = (projects: ProjectService = f.t.projects) => ({
	...f.t,
	projects,
	geocoder: { enabled: true },
	config: { publicUrl: 'https://localnetwork.test' }
});

const opened = async (did?: string, projects?: ProjectService) =>
	loadRoute<PageServerData>(load, {
		app: app(projects),
		viewer: did ? await f.t.viewer(did) : null,
		url: '/for-promoters'
	});

it("describes the page for search engines and link previews at the site's own address", async () => {
	expect(await opened()).toMatchObject({
		geocoder: true,
		meta: {
			title: 'For promoters',
			description:
				'Find DJs by sound, distance and budget, get a shortlist for your night, and book them in private threads.',
			url: 'https://localnetwork.test/for-promoters'
		}
	});
});

it("shows a real listing, then the matcher's shortlist for its sound near its city", async () => {
	await f.t.projectAdmin.create({
		actor: await f.t.viewer(ADMIN),
		profile: {
			name: 'Filled In',
			kind: 'solo',
			tagline: 'Late-night jungle',
			genres: ['jungle'],
			fees: [{ tier: 'established', currency: 'GBP', minAmount: 300 }],
			location: { city: 'Leeds', country: 'GB', latE6: 53_800_000, lngE6: -1_550_000 }
		}
	});

	const data = await opened();

	expect(await data.directory).toMatchObject({ example: { profile: { name: 'Filled In' } }, djs: 2, cities: 2 });
	expect(await data.shortlist).toMatchObject({
		genre: 'jungle',
		place: 'Leeds, GB',
		href: '/match?go=1&genres=jungle&radius=50&place=Leeds%2C+GB&lat=53.8&lng=-1.55',
		results: [{ candidate: { profile: { name: 'Filled In' } }, reasons: expect.arrayContaining(['Plays jungle']) }]
	});
});

it('searches by sound alone for a listing with no coordinates', async () => {
	const shortlist = await (await opened()).shortlist;

	expect(shortlist).toMatchObject({ genre: 'techno', place: null, href: '/match?go=1&genres=techno&radius=50' });
	expect(shortlist?.results.map((result) => result.candidate.slug)).toEqual([f.project.slug]);
});

it('has an empty shortlist while nobody is listed', async () => {
	vi.spyOn(f.t.projects, 'recent').mockResolvedValue([]);

	const data = await opened();

	expect(await data.directory).toMatchObject({ example: null });
	expect(await data.shortlist).toEqual({ genre: null, place: null, href: '/match', results: [] });
});

it('still opens while HappyView is down', async () => {
	const offline = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response('Service Unavailable', { status: 503 })
	});

	const data = await opened(PROMOTER, new ProjectService(f.t.core, offline));

	expect(await data.directory).toBeNull();
	expect(await data.shortlist).toBeNull();
});

it('knows whether someone signed in has the promoter section on', async () => {
	await f.t.profiles.save(await f.t.viewer(PROMOTER), {
		basics: { name: 'Pat' },
		promoter: {},
		avatar: 'keep',
		swap: null
	});

	expect(await (await opened()).promoting).toBe(false);
	expect(await (await opened(PROMOTER)).promoting).toBe(true);
	expect(await (await opened(STRANGER)).promoting).toBe(false);
});
