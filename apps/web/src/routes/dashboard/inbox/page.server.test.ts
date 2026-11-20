import { afterEach, beforeEach, expect, it } from 'vitest';
import { silentLogger } from '#lib/server/logger.ts';
import { NotificationChannels } from '#lib/server/notifications/channels.ts';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import { actions, load } from './+page.server.ts';
import type { PageServerData } from './$types';

const DJ = 'did:plc:dj';

const PROMOTER = 'did:plc:promoter';

let t: Awaited<ReturnType<typeof createTestServices>>;

let notificationChannels: NotificationChannels;

beforeEach(async () => {
	t = await createTestServices();
	notificationChannels = new NotificationChannels({
		db: t.db,
		logger: silentLogger,
		limits: {
			account: new RateLimiter(5, 5 / 86_400),
			accountAddress: new RateLimiter(3, 3 / 86_400),
			address: new RateLimiter(10, 10 / 86_400),
			all: new RateLimiter(100, 100 / 3600)
		}
	});
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The app the inbox reads: the test services, with the channels people send their inbox on by, `sends` set up. */
const app = (sends: ('email' | 'dm')[] = []) => ({
	...t,
	notificationChannels,
	outbox: { configured: sends.filter((channel) => channel === 'email') },
	dm: sends.includes('dm') ? {} : null
});

/** Puts `count` booking inquiries in the DJ's inbox, a minute apart, the newest last. */
async function inquiries(count: number) {
	const start = Date.now() - count * 60_000;

	for (let i = 0; i < count; i++) {
		await t.notifications.notify({
			kind: 'booking.inquiry',
			to: [DJ],
			key: `inquiry-${i}`,
			subject: `at://${PROMOTER}/thread/${i}`,
			data: { skey: `thread${i}`, title: `Show ${i}`, projectName: 'Neon Tide', from: 'promoter' },
			href: `/dashboard/bookings/thread${i}`,
			at: new Date(start + i * 60_000).toISOString()
		});
	}

	return t.notifications.list(DJ, { limit: count });
}

/** The DJ's inbox at `url`, the rows awaited. */
const opened = async (url = '/dashboard/inbox', sends: ('email' | 'dm')[] = []) => {
	const data = await loadRoute<PageServerData>(load, { app: app(sends), viewer: await t.viewer(DJ), url });

	return { ...data, ...(await data.inbox) };
};

it('lists a row per thread, the latest first, with nothing older to page to', async () => {
	await inquiries(2);

	const data = await opened();

	expect(data.rows.map((row) => [row.title, row.summary])).toEqual([
		['Show 1', 'The promoter sent a booking inquiry'],
		['Show 0', 'The promoter sent a booking inquiry']
	]);
	expect(data.older).toBeUndefined();
	expect(data.paused).toEqual([]);
	expect(data.alsoSent).toEqual([]);
});

it('pages a long inbox fifty at a time', async () => {
	const all = await inquiries(52);

	const first = await opened();

	expect(first.rows).toHaveLength(50);
	expect(first.older).toBe(all[49].id);

	const second = await opened(`/dashboard/inbox?before=${first.older}`);

	expect(second.rows.map((row) => row.id)).toEqual([all[50].id, all[51].id]);
	expect(second.older).toBeUndefined();
});

it('says why a channel is paused, leaving out ones that are on', async () => {
	await notificationChannels.enableDm(DJ, false);

	expect((await opened()).paused).toEqual(['dm_not_allowed']);

	await notificationChannels.enableDm(DJ, true);

	expect((await opened()).paused).toEqual([]);
});

it('says where else the inbox goes only where the site sends and the account would get something', async () => {
	await t.db
		.insertInto('notificationChannel')
		.values(
			(['email', 'dm'] as const).map((channel) => ({
				did: DJ,
				channel,
				status: 'active' as const,
				pausedReason: null,
				address: channel === 'email' ? 'dj@example.test' : null,
				addressReadAt: null,
				topics: JSON.stringify(['bookings']),
				lastSentAt: null,
				confirmTokenHash: null,
				confirmAddressHash: null,
				confirmExpiresAt: null,
				unsubscribeToken: `unsub-${channel}`,
				createdAt: new Date().toISOString(),
				updatedAt: new Date().toISOString()
			}))
		)
		.execute();

	expect((await opened()).alsoSent).toEqual([]);
	expect((await opened(undefined, ['email', 'dm'])).alsoSent).toEqual(['email', 'dm']);

	await notificationChannels.setTopics(DJ, 'email', []);
	expect((await opened(undefined, ['email', 'dm'])).alsoSent).toEqual(['dm']);
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: app(), url: '/dashboard/inbox' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Finbox'
	});
});

/** Opens the inbox item `id` as `did`. */
const open = async (id: string, did = DJ) =>
	callRoute(actions.open, { app: app(), viewer: await t.viewer(did), url: '/dashboard/inbox?/open', form: { id } });

it('opens an item where it leads and marks it read', async () => {
	const [newest] = await inquiries(2);

	await expect(open(newest.id)).rejects.toMatchObject({ status: 303, location: '/dashboard/bookings/thread1' });
	expect((await t.notifications.list(DJ)).map((item) => item.read)).toEqual([true, false]);
});

it("stays on the inbox for an item that's gone, someone else's, or not named", async () => {
	const [newest] = await inquiries(1);

	await expect(open('3lgone')).rejects.toMatchObject({ status: 303, location: '/dashboard/inbox' });
	await expect(open(newest.id, PROMOTER)).rejects.toMatchObject({ location: '/dashboard/inbox' });
	await expect(open('')).rejects.toMatchObject({ location: '/dashboard/inbox' });
	expect(await t.notifications.unreadCount(DJ)).toBe(1);
});

it('marks everything read', async () => {
	await inquiries(3);

	await callRoute(actions.readAll, {
		app: app(),
		viewer: await t.viewer(DJ),
		url: '/dashboard/inbox?/readAll',
		form: {}
	});

	expect(await t.notifications.unreadCount(DJ)).toBe(0);
});
