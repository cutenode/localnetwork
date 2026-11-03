import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import { load } from './+page.server.ts';
import type { PageServerData } from './$types';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The admin grants and takes back a moderator role, and the moderator makes nothing, so only the admin has a log. */
async function adminActs() {
	const admin = await t.viewer(ADMIN);
	await t.staff.grant(admin, MOD, 'moderator');
	await t.staff.revoke(admin, MOD);
	await t.audit.idle();
}

it("lists what the viewer did, newest first, and nobody else's", async () => {
	await adminActs();

	const data = await loadRoute<PageServerData>(load, {
		app: t,
		viewer: await t.viewer(ADMIN),
		url: '/dashboard/activity'
	});

	expect(data.entries.map((entry) => [entry.actorDid, entry.action])).toEqual([
		[ADMIN, 'role.revoke'],
		[ADMIN, 'role.grant']
	]);

	const mine = await loadRoute<PageServerData>(load, {
		app: t,
		viewer: await t.viewer(MOD),
		url: '/dashboard/activity'
	});

	expect(mine.entries).toEqual([]);
});

it('pages back from the entry before `before`, and reads a cursor that is not a number as the first page', async () => {
	await adminActs();
	const viewer = await t.viewer(ADMIN);

	const first = await loadRoute<PageServerData>(load, { app: t, viewer, url: '/dashboard/activity' });
	const [newest, oldest] = first.entries;

	const older = await loadRoute<PageServerData>(load, {
		app: t,
		viewer,
		url: `/dashboard/activity?before=${newest.seq}`
	});

	expect(older.entries.map((entry) => entry.seq)).toEqual([oldest.seq]);

	const garbled = await loadRoute<PageServerData>(load, { app: t, viewer, url: '/dashboard/activity?before=soon' });

	expect(garbled.entries).toHaveLength(2);
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: t, url: '/dashboard/activity?before=4' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Factivity%3Fbefore%3D4'
	});
});
