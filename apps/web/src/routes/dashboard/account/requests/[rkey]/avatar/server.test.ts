import { afterEach, beforeEach, expect, it } from 'vitest';
import { ADMIN, DJ, pressKitServices, STRANGER } from '#lib/server/testing/press-kits.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0]);

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The proposed photo of the request `rkey`, as `did` (signed out without one) asks for it from `app`. */
const fetched = async (did: string | null, rkey: string, app: typeof f.t = f.t) =>
	callRoute(GET, {
		app,
		viewer: did ? await f.t.viewer(did) : null,
		url: `/dashboard/account/requests/${rkey}/avatar`,
		params: { rkey }
	});

/** DJ asks for a new project, with a photo unless `avatar` is false. */
const ask = async (avatar = true) =>
	f.t.requests.submit({
		actor: await f.t.viewer(DJ),
		kind: 'create',
		profile: { name: 'Low Hum', kind: 'duo', genres: ['house'], location: { city: 'Leipzig', country: 'DE' } },
		avatar: avatar ? { bytes: PNG, mimeType: 'image/png' } : undefined
	});

it('streams the proposed photo to the requester, and to staff, kept private', async () => {
	const { rkey } = await ask();

	for (const did of [DJ, ADMIN]) {
		const response = await fetched(did, rkey);

		expect(response.status).toBe(200);
		expect(response.headers.get('content-type')).toBe('image/png');
		expect(response.headers.get('cache-control')).toBe('private, max-age=3600');
		expect(new Uint8Array(await response.arrayBuffer())).toEqual(PNG);
	}
});

it("doesn't show the photo to anyone else, or for a request that isn't there", async () => {
	const { rkey } = await ask();

	await expect(fetched(STRANGER, rkey)).rejects.toMatchObject({ status: 404, body: { message: 'Request not found' } });
	await expect(fetched(DJ, 'missing')).rejects.toMatchObject({ status: 404 });
	await expect(fetched(null, rkey)).rejects.toMatchObject({ status: 303 });
});

it("404s for a request without a photo, or when the app can't read spaces' blobs", async () => {
	const plain = await ask(false);
	const pictured = await ask();

	await expect(fetched(DJ, plain.rkey)).rejects.toMatchObject({ status: 404, body: { message: 'No image' } });
	await expect(
		fetched(DJ, pictured.rkey, { ...f.t, core: { ...f.t.core, fetchSpaceBlob: undefined } })
	).rejects.toMatchObject({ status: 404, body: { message: 'No image' } });
});

it("says the requester's PDS didn't answer when the photo can't be fetched", async () => {
	const request = await ask();
	f.t.net.blobs.delete(request.record.profile!.avatar!.ref.$link);

	await expect(fetched(DJ, request.rkey)).rejects.toMatchObject({
		status: 502,
		body: { message: "Could not fetch the image from the requester's PDS" }
	});
});
