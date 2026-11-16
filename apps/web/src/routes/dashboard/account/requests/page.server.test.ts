import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const DJ = 'did:plc:dj';

const OTHER = 'did:plc:other';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ capable: [DJ, OTHER] });
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** `did` asks staff for a new project named `name`. */
const ask = async (did: string, name: string) =>
	t.requests.submit({
		actor: await t.viewer(did),
		kind: 'create',
		profile: { name, kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
	});

it("lists the viewer's own requests and nobody else's", async () => {
	const own = await ask(DJ, 'Neon Tide');
	await ask(OTHER, 'Low Hum');

	const data = await loadRoute<PageServerData>(load, {
		app: t,
		viewer: await t.viewer(DJ),
		url: '/dashboard/account/requests'
	});

	expect((await data.requests).map((request) => request.uri)).toEqual([own.uri]);
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: t, url: '/dashboard/account/requests' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Faccount%2Frequests'
	});
});
