import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { LayoutServerData } from './$types';
import { load } from './+layout.server.ts';

const ADMIN = 'did:plc:admin';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN], capable: [DJ] });
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/**
 * Calls the layout's load at `url` for a session whose grant now lacks scopes the app asks for, which the server hook
 * gives no viewer.
 */
const loadNeedingReauth = (url: string) =>
	loadRoute<LayoutServerData>(load, { app: t, sessionToken: 'session-token', reauth: true, url });

it('gives someone signed out no viewer and no requests to review', async () => {
	expect(await loadRoute<LayoutServerData>(load, { app: t, url: '/' })).toEqual({
		viewer: null,
		pendingRequests: 0,
		flags: { parityCanary: false }
	});
});

it('counts the pending requests for staff only', async () => {
	await t.requests.submit({
		actor: await t.viewer(DJ),
		kind: 'create',
		profile: { name: 'Night Shift', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } }
	});

	const admin = await t.viewer(ADMIN);
	const dj = await t.viewer(DJ);

	expect(await loadRoute<LayoutServerData>(load, { app: t, viewer: admin, url: '/' })).toEqual({
		viewer: admin,
		pendingRequests: 1,
		flags: { parityCanary: false }
	});
	expect(await loadRoute<LayoutServerData>(load, { app: t, viewer: dj, url: '/' })).toEqual({
		viewer: dj,
		pendingRequests: 0,
		flags: { parityCanary: false }
	});
});

it('sends a session that has to sign in again to the login page, back to where it was', async () => {
	await expect(loadNeedingReauth('/dashboard/planner?tab=shows')).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Fplanner%3Ftab%3Dshows'
	});
});

it('lets a session that has to sign in again load the login page itself', async () => {
	expect(await loadNeedingReauth('/login')).toEqual({
		viewer: null,
		pendingRequests: 0,
		flags: { parityCanary: false }
	});
});

it('hands pages the flags the server hook worked out', async () => {
	const data = await loadRoute<LayoutServerData>(load, { app: t, url: '/', flags: { parityCanary: true } });

	expect(data.flags).toEqual({ parityCanary: true });
});
