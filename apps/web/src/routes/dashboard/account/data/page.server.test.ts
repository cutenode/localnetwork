import { afterEach, beforeEach, expect, it } from 'vitest';
import type { HeldRecords } from '#lib/server/services/held-records.ts';
import { NO_SPACES, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { writtenLog } from '#lib/server/testing/written-log.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const PAGE = '/dashboard/account/data';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

it('lists what the root holds for the viewer', async () => {
	const actor = await f.t.viewer(NO_SPACES);

	const request = await f.t.requests.submit({
		actor,
		kind: 'create',
		profile: { name: 'Proxy', kind: 'solo', genres: ['house'], location: { city: 'Berlin', country: 'DE' } }
	});

	const data = await loadRoute<PageServerData>(load, { app: f.t, viewer: actor, url: PAGE });

	expect((await data.records).map((record) => record.uri)).toEqual([request.uri]);
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: f.t, url: PAGE })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Faccount%2Fdata'
	});
});

it("logs a list that couldn't be read, and still opens the page", async () => {
	const { logger: log, notes } = writtenLog('info');

	// SAFETY: the object inherits every member of the real service; only `list` is its own.
	const heldRecords = Object.assign(Object.create(f.t.heldRecords) as HeldRecords, {
		list: async () => {
			throw new Error('index unreachable');
		}
	});

	const data = await loadRoute<PageServerData>(load, {
		app: { ...f.t, heldRecords },
		viewer: await f.t.viewer(NO_SPACES),
		url: PAGE,
		log
	});

	await expect(data.records).rejects.toThrow('index unreachable');
	expect(notes()).toEqual([
		expect.objectContaining({ level: 'warn', message: 'could not list the records held for someone' })
	]);
});
