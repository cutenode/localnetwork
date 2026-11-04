import { afterEach, beforeEach, expect, it } from 'vitest';
import { BSKY_PROFILE } from '#lib/nsid.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { IndexedRepo } from '#lib/server/happyview/indexed.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { ProfileService } from '#lib/server/services/profiles.ts';
import { ProjectService } from '#lib/server/services/projects.ts';
import { DJ, NO_SPACES, pressKitServices, PROMOTER } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { Viewer } from '#lib/types.ts';
import { load as layout } from './+layout.server.ts';
import { load } from './+page.server.ts';
import type { LayoutServerData, PageServerData } from './$types';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The overview as `did` opens it, under the dashboard layout's data, on `app` (the test services unless given). */
async function overview(did: string, { app = f.t, as = {} }: { app?: object; as?: Partial<Viewer> } = {}) {
	const viewer = { ...(await f.t.viewer(did)), ...as };
	const parent = await loadRoute<LayoutServerData>(layout, { app, viewer, url: '/dashboard' });

	return loadRoute<PageServerData>(load, { app, viewer, url: '/dashboard', parent });
}

/** The promoter asks Neon Tide about `title`. Returns the thread's space. */
const inquire = async (title: string) =>
	f.t.bookings.start({
		actor: await f.t.viewer(PROMOTER),
		projectUri: f.project.uri,
		show: { title },
		message: 'Free?'
	});

/** Saves `did`'s profile with only a performer section. */
const performerProfile = async (did: string) =>
	f.t.profiles.save(await f.t.viewer(did), { basics: { name: 'Neon' }, performer: {}, avatar: 'keep', swap: null });

it("puts the booking threads waiting on the viewer first, and doesn't lead a DJ's overview with the planner", async () => {
	await performerProfile(DJ);
	const answered = await inquire('Answered Night');
	await inquire('Waiting Night');
	await f.t.bookings.message({ actor: await f.t.viewer(DJ), space: answered, text: 'Yes, we can do it' });

	const data = await overview(DJ);

	expect(data.threads.map((thread) => thread.title)).toEqual(['Waiting Night', 'Answered Night']);
	expect(data.waiting).toBe(1);
	expect(data.firstRun).toBeNull();
	expect(await data.books).toBe(false);
	expect(await data.planner).toBeNull();
});

it('shows the five newest threads and requests', async () => {
	for (let i = 1; i <= 6; i++) await inquire(`Night ${i}`);

	const promoter = await f.t.viewer(PROMOTER);

	// Five new projects, as many as can wait for review at once, and a claim.
	for (let i = 1; i <= 5; i++)
		await f.t.requests.submit({ actor: promoter, kind: 'create', profile: { ...f.project.profile, name: `Act ${i}` } });
	await f.t.requests.submit({
		actor: promoter,
		kind: 'claim',
		projectUri: f.project.uri,
		claim: { relationship: 'manager', evidence: 'I book them' }
	});

	const data = await overview(PROMOTER);

	expect(data.threads.map((thread) => thread.title)).toEqual(['Night 6', 'Night 5', 'Night 4', 'Night 3', 'Night 2']);
	expect(data.waiting).toBe(0);
	expect(data.requests).toHaveLength(5);
});

it("offers someone without a profile one filled in from Bluesky, and leads a booker's overview with their planner", async () => {
	const promoter = f.t.net.client(PROMOTER);
	const avatar = await promoter.uploadBlob(new TextEncoder().encode('avatar'), 'image/png');
	await promoter.putRecord({
		collection: BSKY_PROFILE,
		rkey: 'self',
		record: { $type: BSKY_PROFILE, displayName: 'Rooftop Collective', avatar }
	});
	await f.t.planner.save(await f.t.viewer(PROMOTER), f.project.uri);

	const data = await overview(PROMOTER);

	expect(data.firstRun).toEqual({ name: 'Rooftop Collective', hasAvatar: true });
	expect(await data.books).toBe(true);
	expect(await data.planner).toMatchObject({ available: true, saves: [{ project: f.project.uri }] });
});

it('names a first profile after the handle, or leaves it blank, without a Bluesky profile to read', async () => {
	expect((await overview(NO_SPACES, { as: { handle: 'nospaces.test' } })).firstRun).toEqual({
		name: 'nospaces.test',
		hasAvatar: false
	});

	const data = await overview(NO_SPACES);

	expect(data.firstRun).toEqual({ name: '', hasAvatar: false });
	expect(await data.planner).toMatchObject({ available: false });
});

it("doesn't lead a performer's overview with the planner, even without projects", async () => {
	await performerProfile(PROMOTER);

	expect(await (await overview(PROMOTER)).books).toBe(false);
});

it("asks nobody to set up a profile, or plan shows, while the directory can't say what they have", async () => {
	const down = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response(null, { status: 503 })
	});

	const app = {
		...f.t,
		projects: new ProjectService(f.t.core, down),
		profiles: new ProfileService(f.t.core, new IndexedRepo(down, silentLogger, 0))
	};

	const data = await overview(PROMOTER, { app });

	expect(data.firstRun).toBeNull();
	expect(await data.books).toBe(false);
	expect(await data.planner).toBeNull();
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: f.t, url: '/dashboard' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard'
	});
});
