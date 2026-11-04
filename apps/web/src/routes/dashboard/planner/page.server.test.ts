import { afterEach, beforeEach, expect, it } from 'vitest';
import { NO_SPACES, pressKitServices, PROMOTER } from '#lib/server/testing/press-kits.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { actions, load } from './+page.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The promoter's planner with Neon Tide saved and one show planned around it. Returns the show's key. */
async function planned() {
	const promoter = await f.t.viewer(PROMOTER);
	await f.t.planner.save(promoter, f.project.uri, 'Ask about NYE');

	return f.t.planner.saveShow(promoter, {
		details: { title: 'Rooftop Rave', date: '2026-12-31T22:00:00.000Z' },
		lineup: [{ project: f.project.uri, status: 'considering' }]
	});
}

it('loads the saved projects and planned shows, with the projects they name', async () => {
	const rkey = await planned();

	const data = await callRoute(load, { app: f.t, viewer: await f.t.viewer(PROMOTER), url: '/dashboard/planner' });

	expect(data).toMatchObject({
		available: true,
		saves: [{ project: f.project.uri, note: 'Ask about NYE' }],
		shows: [{ rkey, details: { title: 'Rooftop Rave' } }],
		projects: { [f.project.uri]: { slug: f.project.slug } }
	});
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: f.t, url: '/dashboard/planner' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Fplanner'
	});
});

it('takes a project off the planner', async () => {
	await planned();
	const promoter = await f.t.viewer(PROMOTER);

	const result = await callRoute(actions.unsave, {
		app: f.t,
		viewer: promoter,
		url: '/dashboard/planner?/unsave',
		form: { project: f.project.uri }
	});

	expect(result).toEqual({ success: 'Removed' });
	expect(await f.t.planner.saves(promoter)).toEqual([]);
});

it('deletes a planned show', async () => {
	const rkey = await planned();
	const promoter = await f.t.viewer(PROMOTER);

	const result = await callRoute(actions.deleteShow, {
		app: f.t,
		viewer: promoter,
		url: '/dashboard/planner?/deleteShow',
		form: { rkey }
	});

	expect(result).toEqual({ success: 'Show deleted' });
	expect(await f.t.planner.shows(promoter)).toEqual([]);
});

it("says why the planner can't change for an account whose server has no spaces", async () => {
	const viewer = await f.t.viewer(NO_SPACES);
	const unsaved = await callRoute(actions.unsave, { app: f.t, viewer, url: '/', form: { project: f.project.uri } });
	const deleted = await callRoute(actions.deleteShow, { app: f.t, viewer, url: '/', form: { rkey: '3lplanned' } });

	const error = "Your PDS doesn't support atproto spaces yet, so the private planner isn't available";

	expect(unsaved).toMatchObject({ status: 400, data: { error } });
	expect(deleted).toMatchObject({ status: 400, data: { error } });
});
