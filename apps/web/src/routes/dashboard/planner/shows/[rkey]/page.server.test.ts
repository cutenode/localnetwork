import { isRedirect } from '@sveltejs/kit';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { ADMIN, NO_SPACES, pressKitServices, PROMOTER } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { ProjectView } from '#lib/types.ts';
import { actions, load } from './+page.server.ts';
import type { PageServerData } from './$types';

let f: Awaited<ReturnType<typeof pressKitServices>>;

/** A second act, saved to the promoter's planner but not on any show. */
let saved: ProjectView;

beforeEach(async () => {
	f = await pressKitServices();
	saved = await f.t.projectAdmin.create({
		actor: await f.t.viewer(ADMIN),
		profile: { ...f.project.profile, name: 'Low Orbit' }
	});
	await f.t.planner.save(await f.t.viewer(PROMOTER), saved.uri);
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The show editor for `rkey`, opened by `did`. */
const opened = async (rkey: string, did = PROMOTER) =>
	loadRoute<PageServerData>(load, {
		app: f.t,
		viewer: await f.t.viewer(did),
		url: `/dashboard/planner/shows/${rkey}`,
		params: { rkey }
	});

/** Posts the show editor for `rkey` as the promoter. */
const submit = async (rkey: string, form: Record<string, string | string[]>) =>
	callRoute(actions.default, {
		app: f.t,
		viewer: await f.t.viewer(PROMOTER),
		url: `/dashboard/planner/shows/${rkey}`,
		params: { rkey },
		form
	});

/** Where `running` redirects to, as SvelteKit throws it, or null when it doesn't redirect. */
async function redirectOf(running: Promise<unknown>) {
	try {
		await running;
	} catch (thrown) {
		if (isRedirect(thrown)) return thrown;
		throw thrown;
	}

	return null;
}

it('opens a new show with the saved projects to pick from', async () => {
	const data = await opened('new');

	expect(data).toMatchObject({ show: null, isNew: true, saves: [{ project: saved.uri }] });
	expect(Object.keys(data.projects)).toEqual([saved.uri]);
});

it("opens a planned show with its lineup's projects as well as the saved ones", async () => {
	const rkey = await f.t.planner.saveShow(await f.t.viewer(PROMOTER), {
		details: { title: 'Rooftop Rave' },
		lineup: [{ project: f.project.uri, status: 'contacted' }]
	});

	const data = await opened(rkey);

	expect(data).toMatchObject({ isNew: false, show: { rkey, details: { title: 'Rooftop Rave' } } });
	expect(Object.keys(data.projects).sort()).toEqual([saved.uri, f.project.uri].sort());
});

it("says a show that isn't planned isn't found, and why an account without spaces has no planner", async () => {
	await expect(opened('3lmissing')).rejects.toMatchObject({ status: 404, body: { message: 'Show not found' } });
	await expect(opened('new', NO_SPACES)).rejects.toMatchObject({
		status: 400,
		body: { message: "Your PDS doesn't support atproto spaces yet, so the private planner isn't available" }
	});
});

it('lets an unexpected failure through as it is', async () => {
	const broken = { ...f.t, projects: { byUris: () => Promise.reject(new Error('directory down')) } };

	await expect(
		loadRoute<PageServerData>(load, {
			app: broken,
			viewer: await f.t.viewer(PROMOTER),
			url: '/',
			params: { rkey: 'new' }
		})
	).rejects.toThrow('directory down');
});

it('sends someone signed out to sign in', async () => {
	await expect(
		loadRoute<PageServerData>(load, { app: f.t, url: '/dashboard/planner/shows/new', params: { rkey: 'new' } })
	).rejects.toMatchObject({ status: 303, location: '/login?next=%2Fdashboard%2Fplanner%2Fshows%2Fnew' });
});

it('plans a new show with its lineup, and opens it', async () => {
	const redirect = await redirectOf(
		submit('new', {
			title: 'Rooftop Rave',
			date: '2026-12-31',
			slotProject: [f.project.uri, saved.uri, 'at://did:plc:other/xyz.localnetwork.project/3lother'],
			slotStatus: ['confirmed', 'maybe'],
			slotLabel: ['  Headliner  ', ' ']
		})
	);

	const [show] = await f.t.planner.shows(await f.t.viewer(PROMOTER));

	expect(redirect).toMatchObject({ status: 303, location: `/dashboard/planner/shows/${show.rkey}` });
	expect(show.details.title).toBe('Rooftop Rave');
	expect(show.lineup).toEqual([
		{ project: f.project.uri, status: 'confirmed', slot: 'Headliner' },
		{ project: saved.uri, status: 'considering' },
		{ project: 'at://did:plc:other/xyz.localnetwork.project/3lother', status: 'considering' }
	]);
});

it('updates a planned show in place, cutting a long slot label', async () => {
	const promoter = await f.t.viewer(PROMOTER);
	const rkey = await f.t.planner.saveShow(promoter, { details: { title: 'Draft' }, lineup: [] });

	await expect(
		submit(rkey, {
			title: 'Rooftop Rave',
			slotProject: f.project.uri,
			slotStatus: 'contacted',
			slotLabel: 'x'.repeat(80)
		})
	).rejects.toMatchObject({ status: 303, location: `/dashboard/planner/shows/${rkey}` });
	expect(await f.t.planner.shows(promoter)).toMatchObject([
		{
			rkey,
			details: { title: 'Rooftop Rave' },
			lineup: [{ project: f.project.uri, status: 'contacted', slot: 'x'.repeat(64) }]
		}
	]);
});

it('keeps what was typed, lineup included, when the show is refused', async () => {
	const result = await submit('new', { title: '', venue: 'The Lot', slotProject: f.project.uri });

	expect(result).toMatchObject({
		status: 400,
		data: {
			error: 'Give the show a name',
			values: { venue: 'The Lot', lineup: [{ project: f.project.uri, status: 'considering' }] }
		}
	});
	expect(await f.t.planner.shows(await f.t.viewer(PROMOTER))).toEqual([]);
});
