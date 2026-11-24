import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DJ, NO_SPACES, pressKitServices, PROMOTER } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

const opened = async (did: string) =>
	loadRoute<PageServerData>(load, {
		app: { ...f.t, geocoder: { enabled: true } },
		viewer: await f.t.viewer(did),
		url: '/dashboard/promoter'
	});

const ask = async (title: string) =>
	f.t.bookings.start({
		actor: await f.t.viewer(PROMOTER),
		projectUri: f.project.uri,
		show: { title },
		message: 'Free?'
	});

it('lists the inquiries the account sent, not the ones sent to it, up to four', async () => {
	for (const title of ['One', 'Two', 'Three', 'Four', 'Five']) await ask(title);

	const promoter = await opened(PROMOTER);
	const dj = await opened(DJ);

	expect(promoter.geocoder).toBe(true);
	expect(promoter.inquiries).toHaveLength(4);
	expect(promoter.inquiryCount).toBe(5);
	expect(dj.inquiries).toEqual([]);
	expect(dj.inquiryCount).toBe(0);
});

it('streams the planned shows, with how many there are', async () => {
	const promoter = await f.t.viewer(PROMOTER);
	await f.t.planner.saveShow(promoter, { details: { title: 'Rooftop Rave' }, lineup: [{ project: f.project.uri }] });

	expect(await (await opened(PROMOTER)).planner).toMatchObject({
		available: true,
		shows: [{ details: { title: 'Rooftop Rave' }, lineup: [{ project: f.project.uri }] }],
		total: 1
	});
});

it("says the planner is unavailable when the account's server has no spaces", async () => {
	expect(await (await opened(NO_SPACES)).planner).toEqual({ available: false, problem: undefined });
});

it("says what went wrong when the account's server doesn't answer", async () => {
	vi.spyOn(f.t.planner, 'shows').mockRejectedValue(new Error('timed out'));

	expect(await (await opened(PROMOTER)).planner).toEqual({ available: false, problem: 'Your PDS did not respond' });
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: f.t, url: '/dashboard/promoter' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Fpromoter'
	});
});
