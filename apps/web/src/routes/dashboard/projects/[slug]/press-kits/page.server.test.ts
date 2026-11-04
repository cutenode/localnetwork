import { afterEach, beforeEach, expect, it } from 'vitest';
import { DJ, pressKitServices, PROMOTER, STRANGER } from '#lib/server/testing/press-kits.ts';
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

/** The list's path, and its query. */
const path = (query = '') => `/dashboard/projects/${f.project.slug}/press-kits${query}`;

/** What the list loads for DJ at `query`. */
async function listed(query = '') {
	return loadRoute<PageServerData>(load, {
		app: f.t,
		viewer: await f.t.viewer(DJ),
		url: path(query),
		parent: { project: f.project }
	});
}

/** A view of a published press kit that PROMOTER, let in, made. */
async function viewedKit() {
	const epk = await f.publish(DJ, { title: 'Festivals' });
	await f.t.epkViews.record(f.kit(epk), await f.t.viewer(PROMOTER));

	return epk;
}

it('lists the press kits, their drafts and who viewed them for a manager', async () => {
	const epk = await viewedKit();

	const data = await listed();

	expect(data).toMatchObject({ viewerDid: DJ, retentionDays: 90, deleted: false });
	expect(await data.dashboard).toMatchObject({ kits: [{ epk: { record: { title: 'Festivals' } } }], drafts: [] });
	expect(await data.views).toMatchObject([{ viewer: { did: PROMOTER }, epk: { rkey: epk.rkey } }]);
});

it('says when it opens right after a press kit was deleted', async () => {
	const data = await listed('?deleted=1');

	expect(data.deleted).toBe(true);
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: f.t, url: path(), parent: { project: f.project } })).rejects.toMatchObject({
		status: 303,
		location: `/login?next=${encodeURIComponent(path())}`
	});
});

it('deletes a view receipt for a manager', async () => {
	await viewedKit();
	const [view] = await f.t.epkViews.list(f.project);

	const result = await callRoute(actions.forgetView, {
		app: f.t,
		viewer: await f.t.viewer(DJ),
		url: path('?/forgetView'),
		params: { slug: f.project.slug },
		form: { view: view.id }
	});

	expect(result).toEqual({ success: 'View deleted.' });
	expect(await f.t.epkViews.list(f.project)).toEqual([]);
});

it('says when the view to delete is already gone', async () => {
	const result = await callRoute(actions.forgetView, {
		app: f.t,
		viewer: await f.t.viewer(DJ),
		url: path('?/forgetView'),
		params: { slug: f.project.slug },
		form: { view: 'gone' }
	});

	expect(result).toMatchObject({ status: 404, data: { error: 'That view is already gone' } });
});

it("refuses to delete a view for someone who doesn't manage the project", async () => {
	await viewedKit();
	const [view] = await f.t.epkViews.list(f.project);

	await expect(
		callRoute(actions.forgetView, {
			app: f.t,
			viewer: await f.t.viewer(STRANGER),
			url: path('?/forgetView'),
			params: { slug: f.project.slug },
			form: { view: view.id }
		})
	).rejects.toMatchObject({ status: 403 });
	expect(await f.t.epkViews.list(f.project)).toHaveLength(1);
});
