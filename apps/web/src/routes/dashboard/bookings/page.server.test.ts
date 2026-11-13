import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { HappyViewError } from '#lib/server/happyview/client.ts';
import { DJ, pressKitServices, PROMOTER, STRANGER } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import { load } from './+page.server.ts';
import type { PageServerData } from './$types';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The bookings page as `did` opens it. */
const listed = async (did: string) =>
	loadRoute<PageServerData>(load, { app: f.t, viewer: await f.t.viewer(did), url: '/dashboard/bookings' });

it('lists the booking threads the viewer is in, on either side', async () => {
	await f.t.bookings.start({
		actor: await f.t.viewer(PROMOTER),
		projectUri: f.project.uri,
		show: { title: 'Warehouse Night' },
		message: 'Free on the 12th?'
	});

	const [asPromoter, asDj, asStranger] = await Promise.all([PROMOTER, DJ, STRANGER].map(listed));

	expect(asPromoter.threads.map((thread) => thread.title)).toEqual(['Warehouse Night']);
	expect(asDj.threads.map((thread) => thread.skey)).toEqual(asPromoter.threads.map((thread) => thread.skey));
	expect([...(await asDj.names)!.values()].map((project) => project.name)).toEqual(['Neon Tide']);
	expect(asDj.acts).toBe(1);
	expect(asStranger.threads).toEqual([]);
	expect(asStranger.acts).toBe(0);
});

it('shows the threads while the directory is down, and says it has no names for them', async () => {
	await f.t.bookings.start({
		actor: await f.t.viewer(PROMOTER),
		projectUri: f.project.uri,
		show: { title: 'Warehouse Night' },
		message: 'Free on the 12th?'
	});
	vi.spyOn(f.t.directory, 'query').mockRejectedValue(new HappyViewError(503, 'down'));

	const { threads, names } = await listed(DJ);

	expect(threads.map((thread) => thread.title)).toEqual(['Warehouse Night']);
	expect(await names).toBeNull();
});

it("doesn't hold the threads back for a slow directory", async () => {
	await f.t.bookings.start({
		actor: await f.t.viewer(PROMOTER),
		projectUri: f.project.uri,
		show: { title: 'Warehouse Night' },
		message: 'Free on the 12th?'
	});
	vi.spyOn(f.t.directory, 'query').mockReturnValue(new Promise(() => {}));

	const { threads, names } = await listed(DJ);

	expect(threads.map((thread) => thread.title)).toEqual(['Warehouse Night']);
	expect(names).toBeInstanceOf(Promise);
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: f.t, url: '/dashboard/bookings' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Fbookings'
	});
});
