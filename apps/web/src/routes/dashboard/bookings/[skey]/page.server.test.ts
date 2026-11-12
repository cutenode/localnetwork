import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { HappyViewError } from '#lib/server/happyview/client.ts';
import { DJ, pressKitServices, PROMOTER, STRANGER } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import { actions, load } from './+page.server.ts';
import type { PageServerData } from './$types';

let f: Awaited<ReturnType<typeof pressKitServices>>;

/** The booking thread the promoter opened with Neon Tide, and its key. */
let space: string;

let skey: string;

beforeEach(async () => {
	f = await pressKitServices();

	space = await f.t.bookings.start({
		actor: await f.t.viewer(PROMOTER),
		projectUri: f.project.uri,
		show: { title: 'Warehouse Night' },
		message: 'Free on the 12th?'
	});

	skey = space.split('/').at(-1)!;
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** What `did` sees of the thread, as the page loads it. */
const opened = async (did: string, key = skey) =>
	loadRoute<PageServerData>(load, {
		app: f.t,
		viewer: await f.t.viewer(did),
		url: `/dashboard/bookings/${key}`,
		params: { skey: key }
	});

it('opens the thread with its inquiry for both sides', async () => {
	const promoter = await opened(PROMOTER);
	const dj = await opened(DJ);

	expect(promoter.thread).toMatchObject({ skey, status: 'open', title: 'Warehouse Night' });
	expect(promoter.events.map((event) => [event.kind, event.text])).toEqual([['inquiry', 'Free on the 12th?']]);
	expect(dj.thread.space).toBe(space);
});

it('streams the act, and opens the thread without it when the directory is down', async () => {
	await expect(Promise.resolve((await opened(DJ)).project)).resolves.toMatchObject({ name: 'Neon Tide' });

	vi.spyOn(f.t.directory, 'query').mockRejectedValue(new HappyViewError(503, 'down'));
	const down = await opened(PROMOTER);

	expect(down.thread.title).toBe('Warehouse Night');
	await expect(Promise.resolve(down.project)).resolves.toBeNull();
});

it("says a thread that doesn't exist, or that the viewer isn't in, isn't found", async () => {
	await expect(opened(PROMOTER, 'missing')).rejects.toMatchObject({ status: 404 });
	await expect(opened(STRANGER)).rejects.toMatchObject({ status: 404, body: { message: 'Booking thread not found' } });
});

it('sends someone signed out to sign in', async () => {
	await expect(
		callRoute(load, { app: f.t, url: `/dashboard/bookings/${skey}`, params: { skey } })
	).rejects.toMatchObject({ status: 303, location: `/login?next=%2Fdashboard%2Fbookings%2F${skey}` });
});

it('sends a message into the thread', async () => {
	const result = await callRoute(actions.message, {
		app: f.t,
		viewer: await f.t.viewer(DJ),
		url: `/dashboard/bookings/${skey}?/message`,
		params: { skey },
		form: { text: '  Yes, we are free.  ' }
	});

	expect(result).toEqual({ sent: true });
	expect((await opened(PROMOTER)).events.at(-1)).toMatchObject({ kind: 'message', text: 'Yes, we are free.' });
});

it("refuses a message from someone outside the thread, and one to a thread that doesn't exist", async () => {
	const stranger = await f.t.viewer(STRANGER);

	const result = await callRoute(actions.message, {
		app: f.t,
		viewer: stranger,
		url: `/dashboard/bookings/${skey}?/message`,
		params: { skey },
		form: { text: 'Can I come?' }
	});

	expect(result).toMatchObject({ status: 403 });
	expect((await opened(PROMOTER)).events).toHaveLength(1);

	await expect(
		callRoute(actions.message, {
			app: f.t,
			viewer: stranger,
			url: '/dashboard/bookings/missing?/message',
			params: { skey: 'missing' },
			form: { text: 'Hello?' }
		})
	).rejects.toMatchObject({ status: 404 });
});

it('moves the booking on with a note, and says what it is now', async () => {
	const result = await callRoute(actions.status, {
		app: f.t,
		viewer: await f.t.viewer(DJ),
		url: `/dashboard/bookings/${skey}?/status`,
		params: { skey },
		form: { status: 'negotiating', note: 'Can do 2 hours' }
	});

	expect(result).toEqual({ status: 'negotiating' });

	const { thread, events } = await opened(PROMOTER);

	expect(thread.status).toBe('negotiating');
	expect(events.at(-1)).toMatchObject({ kind: 'status', status: 'negotiating', text: 'Can do 2 hours' });
});

it("refuses a status change that isn't the viewer's to make, leaving the booking as it was", async () => {
	const result = await callRoute(actions.status, {
		app: f.t,
		viewer: await f.t.viewer(PROMOTER),
		url: `/dashboard/bookings/${skey}?/status`,
		params: { skey },
		form: { status: 'declined' }
	});

	expect(result).toMatchObject({
		status: 409,
		data: { error: "You can't mark this booking declined while it's open" }
	});
	expect((await opened(PROMOTER)).thread.status).toBe('open');
});
