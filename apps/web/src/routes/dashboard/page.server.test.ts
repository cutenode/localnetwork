import { afterEach, beforeEach, expect, it } from 'vitest';
import { BSKY_PROFILE } from '#lib/nsid.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { IndexedRepo } from '#lib/server/happyview/indexed.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { BookingService } from '#lib/server/services/bookings.ts';
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

	return { ...parent, ...(await loadRoute<PageServerData>(load, { app, viewer, url: '/dashboard', parent })) };
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

it('counts what waits on the viewer, and says what is with others', async () => {
	await performerProfile(DJ);
	const answered = await inquire('Answered Night');
	await inquire('Waiting Night');
	await f.t.bookings.message({ actor: await f.t.viewer(DJ), space: answered, text: 'Yes, we can do it' });

	const data = await overview(DJ);

	expect((await data.needs).map((row) => row.title)).toEqual(['Reply to Waiting Night']);
	expect(await data.summary).toEqual({
		others: '1 booking is waiting on a promoter. Nothing needed from you.',
		hasBookings: true
	});
	expect(await data.firstRun).toBeNull();
});

it("says which requests are with staff, and which nights are next from the account's planner", async () => {
	const promoter = await f.t.viewer(PROMOTER);
	await f.t.requests.submit({ actor: promoter, kind: 'create', profile: { ...f.project.profile, name: 'New Act' } });
	await f.t.planner.saveShow(promoter, {
		details: { title: 'Winter Social', date: '2099-12-12T22:00:00.000Z' },
		lineup: []
	});

	const data = await overview(PROMOTER);

	expect(await data.summary).toEqual({
		others: '1 request is with staff. Nothing needed from you.',
		hasBookings: false
	});
	expect((await data.nights).nights.map((night) => night.title)).toEqual(['Winter Social']);
});

it('offers someone without a profile one filled in from Bluesky', async () => {
	const promoter = f.t.net.client(PROMOTER);
	const avatar = await promoter.uploadBlob(new TextEncoder().encode('avatar'), 'image/png');
	await promoter.putRecord({
		collection: BSKY_PROFILE,
		rkey: 'self',
		record: { $type: BSKY_PROFILE, displayName: 'Rooftop Collective', avatar }
	});

	expect(await (await overview(PROMOTER)).firstRun).toEqual({ name: 'Rooftop Collective', hasAvatar: true });
});

it('names a first profile after the handle, or leaves it blank, without a Bluesky profile to read', async () => {
	expect(await (await overview(NO_SPACES, { as: { handle: 'nospaces.test' } })).firstRun).toEqual({
		name: 'nospaces.test',
		hasAvatar: false
	});

	const data = await overview(NO_SPACES);

	expect(await data.firstRun).toEqual({ name: '', hasAvatar: false });
	expect((await data.nights).nights).toEqual([]);
});

it("asks nobody to set up a profile while the directory can't say what they have, and says nights are missing", async () => {
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

	expect(await data.firstRun).toBeNull();
	expect((await data.nights).failed).toBe(true);
});

it('streams what a slow directory holds up, and keeps booking threads while it is down', async () => {
	await inquire('Warehouse Night');

	let answer = (_response: Response) => {};

	const slow = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: () => new Promise((resolve) => (answer = resolve))
	});

	const app = {
		...f.t,
		bookings: new BookingService(f.t.core, new ProjectService(f.t.core, slow)),
		projects: new ProjectService(f.t.core, slow),
		profiles: new ProfileService(f.t.core, new IndexedRepo(slow, silentLogger, 0))
	};

	const data = await overview(DJ, { app });

	expect(data.needs).toBeInstanceOf(Promise);
	expect(data.nights).toBeInstanceOf(Promise);

	answer(new Response(null, { status: 503 }));
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: f.t, url: '/dashboard' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard'
	});
});
