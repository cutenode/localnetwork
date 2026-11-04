import { afterEach, beforeEach, expect, it } from 'vitest';
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
	expect(asDj.threads.map((thread) => thread.project?.name)).toEqual(['Neon Tide']);
	expect(asStranger.threads).toEqual([]);
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: f.t, url: '/dashboard/bookings' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Fbookings'
	});
});
