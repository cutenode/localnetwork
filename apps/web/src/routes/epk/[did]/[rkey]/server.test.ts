import { afterEach, beforeEach, expect, it } from 'vitest';
import { ROOT } from '#lib/server/testing/core.ts';
import { DJ, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

it('sends an old press kit link to where the press kit lives now, keeping its query', async () => {
	const epk = await f.publish(DJ);

	await expect(
		callRoute(GET, { app: f.t, url: `/epk/${ROOT}/${epk.rkey}?ref=email`, params: { did: ROOT, rkey: epk.rkey } })
	).rejects.toMatchObject({ status: 308, location: `/projects/${f.project.slug}/press-kits/${epk.rkey}?ref=email` });
});

it('answers a link to a press kit that is gone with a 404', async () => {
	await expect(
		callRoute(GET, { app: f.t, url: `/epk/${DJ}/3lbnxyk5m2s2a`, params: { did: DJ, rkey: '3lbnxyk5m2s2a' } })
	).rejects.toMatchObject({ status: 404 });
});
