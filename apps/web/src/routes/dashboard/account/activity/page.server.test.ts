import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { AuditLog } from '#lib/server/audit/log.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { writtenLog } from '#lib/server/testing/written-log.ts';
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

/** What `did` sees on the page, once it's all here. */
async function activity(did: string, query = '') {
	const data = await loadRoute<PageServerData>(load, {
		app: t,
		viewer: await t.viewer(did),
		url: `/dashboard/account/activity${query}`
	});

	return data.activity;
}

/** The admin grants and takes back a moderator role, and the moderator makes nothing, so only the admin has a log. */
async function adminActs() {
	const admin = await t.viewer(ADMIN);
	await t.staff.grant(admin, MOD, 'moderator');
	await t.staff.revoke(admin, MOD);
	await t.audit.idle();
}

it("lists what the viewer did, newest first, and nobody else's", async () => {
	await adminActs();

	const data = await activity(ADMIN);

	expect(data.entries.map((entry) => [entry.actorDid, entry.action])).toEqual([
		[ADMIN, 'role.revoke'],
		[ADMIN, 'role.grant']
	]);

	expect((await activity(MOD)).entries).toEqual([]);
});

it('pages back from the entry before `before`, and reads a cursor that is not a number as the first page', async () => {
	await adminActs();

	const first = await activity(ADMIN);
	const [newest, oldest] = first.entries;

	const older = await activity(ADMIN, `?before=${newest.seq}`);

	expect(older.entries.map((entry) => entry.seq)).toEqual([oldest.seq]);

	const garbled = await activity(ADMIN, '?before=soon');

	expect(garbled.entries).toHaveLength(2);
});

it('offers Older only when there is something older', async () => {
	await adminActs();

	expect((await activity(ADMIN)).older).toBeNull();

	const list = vi.spyOn(t.audit, 'list');
	const entries = await t.audit.list({ actor: ADMIN });
	list.mockResolvedValue(Array.from({ length: 51 }, (_, index) => ({ ...entries[0], seq: 100 - index })));

	const { entries: page, older } = await activity(ADMIN);

	expect(page).toHaveLength(50);
	expect(older).toBe(51);
	expect(list).toHaveBeenLastCalledWith(expect.objectContaining({ limit: 51 }));
});

it('streams a failure for the page to show, and logs it', async () => {
	const { logger: log, notes } = writtenLog('info');

	// SAFETY: the object inherits every member of the real service; only `list` is its own.
	const audit = Object.assign(Object.create(t.audit) as AuditLog, {
		list: async () => {
			throw new Error('disk gone');
		}
	});

	const data = await loadRoute<PageServerData>(load, {
		app: { ...t, audit },
		viewer: await t.viewer(ADMIN),
		url: '/dashboard/account/activity',
		log
	});

	await expect(data.activity).rejects.toThrow('disk gone');
	expect(notes()).toEqual([
		expect.objectContaining({ level: 'warn', message: 'could not read someone their activity' })
	]);
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: t, url: '/dashboard/account/activity?before=4' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Faccount%2Factivity%3Fbefore%3D4'
	});
});
