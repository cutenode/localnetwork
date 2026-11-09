import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ capable: [DJ] });
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The app with the geocoder off and the DJ's sets. */
const app = () => ({ ...t, geocoder: { enabled: false }, sets: t.projects.sets });

/** The new project form, filled in. */
const form = { name: 'Neon Tide', kind: 'duo', genres: 'techno', city: 'Berlin', country: 'de' };

it('sends someone signed out to sign in, and opens the form for someone signed in', async () => {
	await expect(callRoute(load, { app: app(), url: '/dashboard/projects/new' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Fprojects%2Fnew'
	});

	const data = await loadRoute<PageServerData>(load, {
		app: app(),
		viewer: await t.viewer(DJ),
		url: '/dashboard/projects/new'
	});

	expect(data.geocoder).toBe(false);
	expect(await data.sets).toEqual([]);
});

it('asks staff for the new project, and opens the request', async () => {
	const sent = callRoute(actions.default, {
		app: app(),
		viewer: await t.viewer(DJ),
		url: '/dashboard/projects/new',
		form: { ...form, bookingEmail: 'book@neon.test', message: 'We play every month at Halle Nord' }
	});

	await expect(sent).rejects.toMatchObject({ status: 303 });
	const [request] = await t.requests.list({ requester: DJ });
	await expect(sent).rejects.toMatchObject({ location: `/dashboard/requests/${request.rkey}?submitted=1` });
	expect(request?.record).toMatchObject({
		kind: 'create',
		profile: { name: 'Neon Tide', kind: 'duo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
		details: { bookingEmail: 'book@neon.test' },
		message: 'We play every month at Halle Nord'
	});
});

it('keeps what was typed and says what to fix when the project has no genre', async () => {
	const result = await callRoute(actions.default, {
		app: app(),
		viewer: await t.viewer(DJ),
		url: '/dashboard/projects/new',
		form: { ...form, genres: '' }
	});

	expect(result).toMatchObject({
		status: 400,
		data: { error: 'Add at least one genre', values: { name: 'Neon Tide' } }
	});
	expect(await t.requests.list({ requester: DJ })).toEqual([]);
});
