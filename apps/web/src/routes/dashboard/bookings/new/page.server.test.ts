import { afterEach, beforeEach, expect, it } from 'vitest';
import { silentLogger } from '#lib/server/logger.ts';
import { NO_SPACES, pressKitServices, PROMOTER } from '#lib/server/testing/press-kits.ts';
import { inquiryDefaults } from '#lib/shows.ts';
import type { Viewer } from '#lib/types.ts';
import { load } from './+page.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** What the inquiry form opens with for `did` at `query`, with their viewer changed by `as`. */
async function opened(did: string, query: string, as: Partial<Viewer> = {}) {
	const locals = { app: f.t, viewer: { ...(await f.t.viewer(did)), ...as }, log: silentLogger };
	const url = new URL(`https://x.test/dashboard/bookings/new?project=${f.project.slug}&${query}`);

	// SAFETY: the load reads only the URL and locals, and the services are the app's own.
	const data = await load({ url, locals } as never);

	return data && { values: data.values, show: data.show };
}

it('fills the inquiry in from the planner show it was opened from', async () => {
	const promoter = await f.t.viewer(PROMOTER);

	const rkey = await f.t.planner.saveShow(promoter, {
		details: {
			title: 'Rooftop Rave',
			date: '2026-12-12T23:30:00.000Z',
			venue: 'The Lot',
			capacity: 300,
			genres: ['techno']
		},
		lineup: [{ project: f.project.uri, status: 'considering' }]
	});

	expect(await opened(PROMOTER, `show=${rkey}`)).toEqual({
		values: {
			...inquiryDefaults,
			title: 'Rooftop Rave',
			date: '2026-12-12',
			time: '23:30',
			venue: 'The Lot',
			capacity: '300',
			genres: 'techno'
		},
		show: { rkey, title: 'Rooftop Rave' }
	});
});

it("opens with the matcher's date, or empty, when there's no planner show to read", async () => {
	expect(await opened(PROMOTER, 'date=2026-12-12')).toEqual({
		values: { ...inquiryDefaults, date: '2026-12-12' },
		show: null
	});
	expect(await opened(PROMOTER, 'show=missing')).toEqual({ values: inquiryDefaults, show: null });
	// An account without spaces has no planner, so the form opens as if there were none.
	expect(await opened(NO_SPACES, 'show=missing')).toEqual({ values: inquiryDefaults, show: null });
	// A planner that can't be read leaves the form empty rather than failing the page.
	expect(await opened(NO_SPACES, 'show=missing', { spacesCapable: true })).toEqual({
		values: inquiryDefaults,
		show: null
	});
});
