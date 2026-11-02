import { afterEach, beforeEach, expect, it } from 'vitest';
import { silentLogger } from '#lib/server/logger.ts';
import { ADMIN, NO_SPACES, pressKitServices, PROMOTER } from '#lib/server/testing/press-kits.ts';
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

/** The inquiry form's load for `did` at `query` to `slug`, with their viewer changed by `as`. */
async function loaded(did: string, query: string, as: Partial<Viewer> = {}, slug = f.project.slug) {
	const locals = {
		app: { ...f.t, geocoder: { enabled: false } },
		viewer: { ...(await f.t.viewer(did)), ...as },
		log: silentLogger
	};

	const url = new URL(`https://x.test/dashboard/bookings/new?project=${slug}&${query}`);

	// SAFETY: the load reads only the URL and locals, and the services are the app's own.
	return load({ url, locals } as never);
}

/** What the inquiry form opens with: its values, and the planner show they came from. */
async function opened(did: string, query: string, as: Partial<Viewer> = {}) {
	const data = await loaded(did, query, as);

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

it("starts the budget in the currency of the project's first fee, with what the project asks for beside it", async () => {
	const priced = await f.t.projectAdmin.create({
		actor: await f.t.viewer(ADMIN),
		profile: {
			name: 'Priced',
			kind: 'solo',
			genres: ['techno'],
			location: { city: 'Berlin', country: 'DE' },
			fees: [{ tier: 'established', currency: 'EUR', minAmount: 600, maxAmount: 1500 }],
			setTypes: ['peak', 'closing'],
			setLength: { minMinutes: 90, maxMinutes: 240 }
		}
	});

	const data = await loaded(PROMOTER, '', {}, priced.slug);

	expect(data?.values.currency).toBe('EUR');
	expect(data?.asks).toEqual([
		'Fee: Established · €600–€1,500.',
		'Slots: Peak time, Closing.',
		'Set length: 90–240 minutes.'
	]);
	// The demo project asks for nothing, so nothing shows beside the budget.
	expect((await loaded(PROMOTER, ''))?.asks).toEqual([]);
});
