import { afterEach, beforeEach, expect, it } from 'vitest';
import type { EpkPhoto } from '#lib/epk.ts';
import { ADMIN, DJ, FRIEND, pressKitServices, STRANGER, values } from '#lib/server/testing/press-kits.ts';
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

/** What the photo route answers `did` (signed out without one) with for `photo` in the press kit `rkey`. */
const get = async (did: string | null, rkey: string, photo: EpkPhoto, { query = '', slug = f.project.slug } = {}) =>
	callRoute(GET, {
		app: f.t,
		viewer: did ? await f.t.viewer(did) : null,
		url: `/projects/${slug}/press-kits/${rkey}/photos/${photo.image.ref.$link}${query}`,
		params: { slug, rkey, cid: photo.image.ref.$link }
	});

/** The bytes a response carries, as text. */
const body = async (response: Response) => new TextDecoder().decode(await response.arrayBuffer());

/** A press kit with one photo for everyone and one only for people with access. */
async function kitWithPhotos() {
	const [open, held] = [await f.photo(DJ, 'Live at the warehouse'), await f.photo(DJ, 'Soundcheck')];
	const epk = await f.publish(DJ, { photos: [open], content: { ...values().content, photos: [held] } });

	return { epk, open, held };
}

it('serves a public photo to anyone, cached for a day', async () => {
	const { epk, open } = await kitWithPhotos();

	const response = await get(null, epk.rkey, open);

	expect(response.headers.get('cache-control')).toBe('public, max-age=86400, immutable');
	expect(response.headers.get('content-disposition')).toBe('inline');
	expect(await body(response)).toBe('Live at the warehouse');
});

it('serves a photo for people with access to someone let in, privately and as a download when asked', async () => {
	const { epk, held } = await kitWithPhotos();
	await f.t.epks.invite(await f.t.viewer(DJ), f.kit(epk), FRIEND);

	const response = await get(FRIEND, epk.rkey, held, { query: '?download' });

	expect(response.headers.get('cache-control')).toBe('private, max-age=3600');
	expect(response.headers.get('content-disposition')).toBe('attachment');
	expect(await body(response)).toBe('Soundcheck');
});

it('serves a photo for people with access to a manager', async () => {
	const { epk, held } = await kitWithPhotos();

	expect(await body(await get(DJ, epk.rkey, held))).toBe('Soundcheck');
});

it('keeps a photo for people with access from everyone else', async () => {
	const { epk, held } = await kitWithPhotos();

	await expect(get(STRANGER, epk.rkey, held)).rejects.toMatchObject({ status: 404 });
	await expect(get(null, epk.rkey, held)).rejects.toMatchObject({ status: 404 });
});

it("shows a hidden project's photos only to its managers and staff", async () => {
	const { epk, open } = await kitWithPhotos();
	await f.t.projectAdmin.setStatus({ actor: await f.t.viewer(ADMIN), project: f.project, status: 'hidden' });

	await expect(get(null, epk.rkey, open)).rejects.toMatchObject({ status: 404 });
	await expect(get(STRANGER, epk.rkey, open)).rejects.toMatchObject({ status: 404 });
	expect(await body(await get(DJ, epk.rkey, open))).toBe('Live at the warehouse');
	expect(await body(await get(ADMIN, epk.rkey, open))).toBe('Live at the warehouse');
});

it("answers a photo of a press kit or project that isn't there with a 404", async () => {
	const { epk, open } = await kitWithPhotos();

	await expect(get(null, '3lbnxyk5m2s2a', open)).rejects.toMatchObject({ status: 404 });
	await expect(get(null, epk.rkey, open, { slug: 'nobody' })).rejects.toMatchObject({ status: 404 });
});
