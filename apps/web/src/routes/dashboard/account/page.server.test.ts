import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { writtenLog } from '#lib/server/testing/written-log.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

const notificationChannels = { view: async () => null };

beforeEach(async () => {
	t = await createTestServices({ capable: [DJ] });
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

it("says where each page stands from the viewer's own records", async () => {
	const actor = await t.viewer(DJ);
	await t.requests.submit({
		actor,
		kind: 'create',
		profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
	});
	await t.audit.idle();

	const data = await loadRoute<PageServerData>(load, {
		app: { ...t, notificationChannels },
		viewer: actor,
		url: '/dashboard/account'
	});

	expect(await data.channels).toEqual([null, null]);
	expect((await data.latest)?.map((entry) => entry.action)).toEqual(['request.submit']);
	expect((await data.requests)?.map((request) => request.status)).toEqual(['pending']);
	expect(await data.held).toEqual([]);
});

it('loses only the line of a source that fails, and logs it', async () => {
	const { logger: log, notes } = writtenLog('info');

	// SAFETY: each object inherits every member of the real service; only `list` is its own.
	const requests = Object.assign(Object.create(t.requests) as typeof t.requests, {
		list: async () => {
			throw new Error('disk gone');
		}
	});

	// SAFETY: as above.
	const heldRecords = Object.assign(Object.create(t.heldRecords) as typeof t.heldRecords, { list: async () => [] });

	const data = await loadRoute<PageServerData>(load, {
		app: { ...t, notificationChannels, requests, heldRecords },
		viewer: await t.viewer(DJ),
		url: '/dashboard/account',
		log
	});

	expect(await data.requests).toBeNull();
	expect(await data.held).toEqual([]);
	expect(notes()).toEqual([
		expect.objectContaining({ level: 'warn', message: 'could not read a line for the account page' })
	]);
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: t, url: '/dashboard/account' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Faccount'
	});
});
