import { afterEach, beforeEach, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { DjService } from '#lib/server/services/dj.ts';
import { ADMIN, DJ, FRIEND, NO_SPACES, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute, routeEvent } from '#lib/testing/route-event.ts';
import type { Viewer } from '#lib/types.ts';
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

/** The app, with the site's public URL, DJs' own records, and the sets mixes link. */
const app = () => ({
	...f.t,
	config: { publicUrl: 'https://localnetwork.test' },
	dj: new DjService(f.t.indexedRepo, silentLogger),
	sets: f.t.projects.sets
});

/** Neon Tide's page as `viewer` loads it. */
const page = (viewer: Viewer | null = null) =>
	loadRoute<PageServerData>(load, {
		app: app(),
		viewer,
		params: { slug: f.project.slug },
		url: `/projects/${f.project.slug}`
	});

/** The action `name` on `slug`'s page, as `viewer` posts it. */
const post = (name: keyof typeof actions, viewer: Viewer | null, slug = f.project.slug) =>
	callRoute(actions[name], { app: app(), viewer, params: { slug }, url: `/projects/${slug}?/${name}`, form: {} });

it('shows an active project to someone signed out, streaming its shows and followers', async () => {
	const { loadData, locals } = routeEvent({
		app: app(),
		params: { slug: f.project.slug },
		url: `/projects/${f.project.slug}`
	});

	const data = await loadData<PageServerData>(load);

	expect(data).toMatchObject({ details: null, saved: null, pendingClaim: null, pendingUpdate: null, sets: undefined });
	expect(data.project.uri).toBe(f.project.uri);
	expect(await data.shows).toEqual([]);
	expect(await data.follow).toEqual({ followers: 0, following: false });
	expect(data.meta.url).toBe(`https://localnetwork.test/projects/${f.project.slug}`);
	expect(locals).toMatchObject({ crawlerHead: expect.stringContaining('application/ld+json') });
});

it('tells a viewer the project is in their planner, that they follow it, and about their open claim', async () => {
	const friend = await f.t.viewer(FRIEND);

	await f.t.planner.save(friend, f.project.uri);
	await f.t.follows.follow(friend, f.project);
	await f.t.requests.submit({
		actor: friend,
		kind: 'claim',
		projectUri: f.project.uri,
		claim: { relationship: 'self', evidence: 'I play as Neon Tide; see my Bluesky bio' }
	});

	const data = await page(friend);

	expect(data.saved).toBe(true);
	expect(await data.follow).toEqual({ followers: 1, following: true });
	expect(data.pendingClaim).toMatchObject({ kind: 'claim', requester: { did: FRIEND } });
	expect(data.pendingUpdate).toBeNull();
});

it('shows a manager the changes they have waiting for review, and the booking details', async () => {
	const dj = await f.t.viewer(DJ);

	await f.t.requests.submit({
		actor: dj,
		kind: 'update',
		projectUri: f.project.uri,
		profile: { ...f.project.profile, tagline: 'Rolling techno for late rooms' }
	});

	const data = await page(dj);

	expect(data.pendingUpdate).toMatchObject({ kind: 'update', requester: { did: DJ } });
	expect(data.pendingClaim).toBeNull();
	expect(data.details).toEqual({});
	expect(data.saved).toBe(false);
});

it("leaves whether it's saved unknown when the viewer's planner can't be read", async () => {
	const lapsed = { ...(await f.t.viewer(NO_SPACES)), spacesCapable: true };
	const data = await page(lapsed);

	expect(data.saved).toBeNull();
});

it("streams the sets the project's mixes link", async () => {
	const set = await f.t.net.client(DJ).putRecord({
		collection: COLLECTION.djSet,
		rkey: '3kset',
		record: {
			$type: COLLECTION.djSet,
			title: 'Closing',
			date: '2026-09-01',
			tracks: [{}],
			createdAt: '2026-09-02T00:00:00Z'
		}
	});

	await f.t.projectAdmin.update({
		actor: await f.t.viewer(ADMIN),
		project: f.project,
		profile: {
			...f.project.profile,
			mixes: [
				{ title: 'Late Room', url: 'https://mixes.test/late-room' },
				{ title: 'Warehouse', url: 'https://mixes.test/warehouse', set }
			]
		}
	});

	const data = await page();

	expect([...((await data.sets)?.keys() ?? [])]).toEqual([set.uri]);
});

it('shows a hidden project to its managers, without following and kept out of search, and to no one else', async () => {
	await f.t.projectAdmin.setStatus({ actor: await f.t.viewer(ADMIN), project: f.project, status: 'hidden' });

	const { loadData, locals } = routeEvent({
		app: app(),
		viewer: await f.t.viewer(DJ),
		params: { slug: f.project.slug },
		url: `/projects/${f.project.slug}`
	});

	const data = await loadData<PageServerData>(load);

	expect(data.follow).toBeUndefined();
	expect(data.meta.noindex).toBe(true);
	expect(locals).not.toHaveProperty('crawlerHead');
	await expect(page()).rejects.toMatchObject({ status: 404 });
});

it("saves the project to the viewer's planner, and takes it out again", async () => {
	const friend = await f.t.viewer(FRIEND);

	expect(await post('save', friend)).toEqual({ success: 'Saved to your planner' });
	expect((await f.t.planner.saves(friend)).map((save) => save.project)).toEqual([f.project.uri]);
	expect(await post('unsave', friend)).toEqual({ success: 'Removed from your planner' });
	expect(await f.t.planner.saves(friend)).toEqual([]);
});

it('follows the project, and unfollows it again', async () => {
	const friend = await f.t.viewer(FRIEND);

	expect(await post('follow', friend)).toEqual({ followed: true });
	expect(await f.t.follows.state(f.project, FRIEND)).toEqual({ followers: 1, following: true });
	expect(await post('unfollow', friend)).toEqual({ followed: false });
	expect(await f.t.follows.state(f.project, FRIEND)).toEqual({ followers: 0, following: false });
});

it('sends someone signed out to sign in before saving or following', async () => {
	for (const name of ['save', 'unsave', 'follow', 'unfollow'] as const) {
		await expect(post(name, null)).rejects.toMatchObject({
			status: 303,
			location: `/login?next=${encodeURIComponent(`/projects/${f.project.slug}?/${name}`)}`
		});
	}
});

it("answers 404 for a project that isn't there", async () => {
	const friend = await f.t.viewer(FRIEND);

	expect(await post('save', friend, 'nobody')).toMatchObject({ status: 404, data: { error: 'Project not found' } });
	expect(await post('unsave', friend, 'nobody')).toMatchObject({ status: 404, data: { error: 'Project not found' } });
	await expect(post('follow', friend, 'nobody')).rejects.toMatchObject({ status: 404 });
	await expect(post('unfollow', friend, 'nobody')).rejects.toMatchObject({ status: 404 });
});

it('refuses to follow a project that is no longer active', async () => {
	await f.t.projectAdmin.setStatus({ actor: await f.t.viewer(ADMIN), project: f.project, status: 'hidden' });

	expect(await post('follow', await f.t.viewer(FRIEND))).toMatchObject({
		status: 404,
		data: { error: 'That project is not available' }
	});
});
