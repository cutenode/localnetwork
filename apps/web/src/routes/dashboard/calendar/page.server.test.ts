import { afterEach, beforeEach, expect, it } from 'vitest';
import { pressKitServices, PROMOTER } from '#lib/server/testing/press-kits.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
import { load as layout } from '../+layout.server.ts';
import type { LayoutServerData } from '../$types';
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

const inDays = (days: number) => new Date(Date.now() + days * 86_400_000).toISOString();

it('lists every night for a year ahead, not just the first few', async () => {
	const promoter = await f.t.viewer(PROMOTER);

	for (const days of [3, 10, 20, 40, 80, 160, 300, 400]) {
		await f.t.planner.saveShow(promoter, { details: { title: `In ${days} days`, date: inDays(days) }, lineup: [] });
	}

	const parent = await loadRoute<LayoutServerData>(layout, { app: f.t, viewer: promoter, url: '/dashboard' });

	const data = await loadRoute<PageServerData>(load, {
		app: f.t,
		viewer: promoter,
		url: '/dashboard/calendar',
		parent,
		flags: { dashboardCalendar: true }
	});

	const { nights, more, failed } = await data.nights;

	expect(nights.map((night) => night.title)).toEqual([3, 10, 20, 40, 80, 160, 300].map((days) => `In ${days} days`));
	expect(more).toBe(0);
	expect(failed).toBe(false);
});

it("isn't there while dashboardCalendar is off for the viewer", async () => {
	const promoter = await f.t.viewer(PROMOTER);
	const parent = await loadRoute<LayoutServerData>(layout, { app: f.t, viewer: promoter, url: '/dashboard' });

	await expect(
		loadRoute(load, { app: f.t, viewer: promoter, url: '/dashboard/calendar', parent })
	).rejects.toMatchObject({
		status: 404
	});
});
