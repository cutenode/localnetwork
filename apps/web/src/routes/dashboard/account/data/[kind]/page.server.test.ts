import { afterEach, beforeEach, expect, it } from 'vitest';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { HeldRecords } from '#lib/server/services/held-records.ts';
import { NO_SPACES, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { writtenLog } from '#lib/server/testing/written-log.ts';
import type { HeldRecordView } from '#lib/types.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import { actions, load } from './+page.server.ts';

const PAGE = '/dashboard/account/data/request';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The records page for `kind`, as `viewer`. */
const open = (init: Parameters<typeof loadRoute>[1], kind: string) =>
	loadRoute<{ kind: string; records: Promise<HeldRecordView[]> }>(load, { ...init, params: { kind } });

it('lists what the root holds for the viewer of the one kind', async () => {
	const uri = await submitted();

	const requests = await open(await as(NO_SPACES, PAGE), 'request');
	expect((await requests.records).map((record) => record.uri)).toEqual([uri]);

	const bookings = await open(await as(NO_SPACES, PAGE), 'booking');
	expect(await bookings.records).toEqual([]);
});

it("doesn't have a page for what isn't a kind", async () => {
	await expect(open(await as(NO_SPACES, PAGE), 'nonsense')).rejects.toMatchObject({ status: 404 });
});

it('sends someone signed out to sign in, on the page and both actions', async () => {
	const signIn = { status: 303, location: '/login?next=%2Fdashboard%2Faccount%2Fdata%2Frequest' };

	await expect(callRoute(load, { app: f.t, url: PAGE, params: { kind: 'request' } })).rejects.toMatchObject(signIn);
	await expect(callRoute(actions.delete, { app: f.t, url: PAGE, form: { uri: 'x' } })).rejects.toMatchObject(signIn);
	await expect(callRoute(actions.deleteAll, { app: f.t, url: PAGE, form: { kind: 'request' } })).rejects.toMatchObject(
		signIn
	);
});

it("logs a list that couldn't be read, and still opens the page", async () => {
	const { logger: log, notes } = writtenLog('info');

	// SAFETY: the object inherits every member of the real service; only `list` is its own.
	const heldRecords = Object.assign(Object.create(f.t.heldRecords) as HeldRecords, {
		list: async () => {
			throw new Error('index unreachable');
		}
	});

	const data = await open({ ...(await as(NO_SPACES, PAGE)), app: { ...f.t, heldRecords }, log }, 'request');

	await expect(data.records).rejects.toThrow('index unreachable');
	expect(notes()).toEqual([
		expect.objectContaining({ level: 'warn', message: 'could not list the records held for someone' })
	]);
});

it('deletes one held record', async () => {
	const uri = await submitted();

	const result = await callRoute(actions.delete, {
		...(await as(NO_SPACES, `${PAGE}?/delete`)),
		form: { uri }
	});

	expect(result).toEqual({ success: 'Deleted.' });
	expect(await f.t.heldRecords.list(await f.t.viewer(NO_SPACES))).toEqual([]);
});

it("says what's wrong when the record isn't the viewer's to delete", async () => {
	const result = await callRoute(actions.delete, {
		...(await as(NO_SPACES, `${PAGE}?/delete`)),
		form: { uri: 'at://did:plc:root/space/x/y/did:plc:root/computer.localnetwork.request/nope' }
	});

	expect(result).toMatchObject({ status: 404 });
});

it('deletes every record of a kind, and says so', async () => {
	await submitted();
	await submitted();

	const all = await callRoute(actions.deleteAll, {
		...(await as(NO_SPACES, `${PAGE}?/deleteAll`)),
		form: { kind: 'request' }
	});

	expect(all).toEqual({ success: 'Deleted 2 records.' });
	expect(await f.t.heldRecords.list(await f.t.viewer(NO_SPACES))).toEqual([]);
});

it("says there's nothing to delete for a kind with none held, or one that isn't a kind", async () => {
	for (const kind of ['request', 'nonsense']) {
		const result = await callRoute(actions.deleteAll, {
			...(await as(NO_SPACES, `${PAGE}?/deleteAll`)),
			form: { kind }
		});

		expect(result).toEqual({ success: 'Nothing to delete.' });
	}
});

it("doesn't spend the viewer's deletes on a kind that isn't one", async () => {
	await submitted();
	// One delete a viewer, never refilled.
	const heldRecords = new HeldRecords(f.t.core, f.t.requests, f.t.projects, new RateLimiter(1, 0));
	const init = { ...(await as(NO_SPACES, `${PAGE}?/deleteAll`)), app: { ...f.t, heldRecords } };

	await callRoute(actions.deleteAll, { ...init, form: { kind: 'nonsense' } });

	expect(await callRoute(actions.deleteAll, { ...init, form: { kind: 'request' } })).toEqual({
		success: 'Deleted 1 record.'
	});
});

// Helpers

/** The route's event as `did`, at `url`. */
const as = async (did: string, url: string) => ({ app: f.t, viewer: await f.t.viewer(did), url });

/** A request the root writes for NO_SPACES, whose PDS can't write to spaces. Returns its URI. */
async function submitted(): Promise<string> {
	const request = await f.t.requests.submit({
		actor: await f.t.viewer(NO_SPACES),
		kind: 'create',
		profile: { name: 'Proxy', kind: 'solo', genres: ['house'], location: { city: 'Berlin', country: 'DE' } }
	});

	return request.uri;
}
