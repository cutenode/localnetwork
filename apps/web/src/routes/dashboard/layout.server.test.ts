import pino from 'pino';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { IndexedRepo } from '#lib/server/happyview/indexed.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { ProfileService } from '#lib/server/services/profiles.ts';
import { ProjectService } from '#lib/server/services/projects.ts';
import { ROOT } from '#lib/server/testing/core.ts';
import { DJ, pressKitServices, PROMOTER } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import { load } from './+layout.server.ts';
import type { LayoutServerData } from './$types';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The dashboard layout as `did` opens it, on the booking thread `skey` when given. */
const opened = async (did: string, { app = {}, skey }: { app?: object; skey?: string } = {}) =>
	loadRoute<LayoutServerData>(load, {
		app: { ...f.t, ...app },
		viewer: await f.t.viewer(did),
		url: skey ? `/dashboard/bookings/${skey}` : '/dashboard',
		params: skey ? { skey } : {}
	});

/** The promoter asks Neon Tide about a show, which tells its managers. Returns the thread's key. */
async function inquiry() {
	const space = await f.t.bookings.start({
		actor: await f.t.viewer(PROMOTER),
		projectUri: f.project.uri,
		show: { title: 'Warehouse Night' },
		message: 'Free on the 12th?'
	});

	return space.split('/').at(-1)!;
}

it("streams the projects the account manages and its profile's sections, with its unread count", async () => {
	await inquiry();
	await f.t.profiles.save(await f.t.viewer(DJ), {
		basics: { name: 'Neon' },
		performer: {},
		avatar: 'keep',
		swap: null
	});

	const data = await opened(DJ);

	expect((await data.projects)?.map((project) => project.profile.name)).toEqual(['Neon Tide']);
	expect(await data.sections).toEqual({ performer: true, promoter: false });
	expect(data.unread).toBe(1);
});

it('shows every section for an account without a profile', async () => {
	const data = await opened(PROMOTER);

	expect(await data.projects).toEqual([]);
	expect(await data.sections).toBeNull();
	expect(data.unread).toBe(0);
});

it("marks what the inbox said about a booking thread read as it's opened, and nothing for a thread that isn't there", async () => {
	const skey = await inquiry();

	expect((await opened(DJ, { skey: 'missing' })).unread).toBe(1);
	expect((await opened(DJ, { skey })).unread).toBe(0);
});

it('opens with the projects and sections left out while the directory is down', async () => {
	const down = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response(null, { status: 503 })
	});

	const app = {
		projects: new ProjectService(f.t.core, down),
		profiles: new ProfileService(f.t.core, new IndexedRepo(down, silentLogger, 0))
	};

	const data = await opened(DJ, { app });

	expect(await data.projects).toBeNull();
	expect(await data.sections).toBeNull();
});

it("moves the account's legacy press kits to the root without holding the page up", async () => {
	const legacy = await f.legacy(DJ);

	await opened(DJ);

	await vi.waitFor(() => expect(f.t.net.records.get(`${ROOT}|${COLLECTION.epk}|${legacy.rkey}`)).toBeDefined());
});

it("still opens when legacy press kits can't be moved, and says why in the log", async () => {
	/** Each line the route logged, as pino writes it. */
	const logs: { level: string; msg: string; err?: { message: string } }[] = [];

	const log = pino(
		{ level: 'info', base: null, timestamp: false, formatters: { level: (level) => ({ level }) } },
		{ write: (line: string) => void logs.push(JSON.parse(line)) }
	);

	const epks = { migrateLegacy: () => Promise.reject(new Error('PDS unreachable')) };

	const data = await loadRoute<LayoutServerData>(load, {
		app: { ...f.t, epks },
		viewer: await f.t.viewer(DJ),
		url: '/dashboard',
		log
	});

	expect(data.unread).toBe(0);
	expect(logs).toContainEqual(
		expect.objectContaining({
			level: 'warn',
			msg: 'could not move legacy press kits',
			err: expect.objectContaining({ message: 'PDS unreachable' })
		})
	);
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: f.t, url: '/dashboard', params: {} })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard'
	});
});
