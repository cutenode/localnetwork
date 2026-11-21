import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { writtenLog } from '#lib/server/testing/written-log.ts';
import type { ChannelView } from '#lib/types.ts';
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

	expect(await data.channels).toEqual(['off', 'off']);
	expect((await data.latest)?.map((entry) => entry.action)).toEqual(['request.submit']);
	expect((await data.requests)?.map((request) => request.status)).toEqual(['pending']);
	expect(await data.held).toEqual([]);
});

it("says a channel they have is paused while it's paused, or while this site doesn't offer it to them", async () => {
	// Arrange
	const email: ChannelView = {
		status: 'active',
		pausedReason: null,
		address: 'dj@neontide.test',
		topics: ['bookings']
	};

	const dm: ChannelView = { status: 'paused', pausedReason: 'dm_not_allowed', address: null, topics: ['bookings'] };
	const views = { view: async (_did: string, channel: string) => (channel === 'email' ? email : dm) };
	const app = { ...t, notificationChannels: views, outbox: { configured: ['email', 'dm'] }, dm: {} };
	const viewer = await t.viewer(DJ);
	const url = '/dashboard/account';

	// Act
	const offered = await loadRoute<PageServerData>(load, {
		app,
		viewer,
		url,
		flags: { inboxEmail: true, inboxDm: true }
	});

	const notOffered = await loadRoute<PageServerData>(load, { app, viewer, url });

	// Assert
	expect([await offered.channels, await notOffered.channels]).toEqual([
		['on', 'paused'],
		['paused', 'paused']
	]);
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
