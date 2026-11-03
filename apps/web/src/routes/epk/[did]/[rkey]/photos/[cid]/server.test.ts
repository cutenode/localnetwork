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

it("sends an old photo link to the photo's place in the press kit now, keeping ?download", async () => {
	const photo = await f.photo(DJ, 'Live at the warehouse');
	const epk = await f.publish(DJ, { photos: [photo] });
	const cid = photo.image.ref.$link;

	await expect(
		callRoute(GET, {
			app: f.t,
			url: `/epk/${ROOT}/${epk.rkey}/photos/${cid}?download`,
			params: { did: ROOT, rkey: epk.rkey, cid }
		})
	).rejects.toMatchObject({
		status: 308,
		location: `/projects/${f.project.slug}/press-kits/${epk.rkey}/photos/${cid}?download`
	});
});

it('answers a photo link of a press kit that is gone with a 404', async () => {
	await expect(
		callRoute(GET, {
			app: f.t,
			url: `/epk/${DJ}/3lbnxyk5m2s2a/photos/bafkreigh2akiscaildc`,
			params: { did: DJ, rkey: '3lbnxyk5m2s2a', cid: 'bafkreigh2akiscaildc' }
		})
	).rejects.toMatchObject({ status: 404 });
});
