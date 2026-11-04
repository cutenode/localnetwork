import { afterEach, beforeEach, expect, it } from 'vitest';
import { CODJ, DJ, pressKitServices, STRANGER, values } from '#lib/server/testing/press-kits.ts';
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

/** What the editor's photo route answers `did` with for `cid` in the press kit `rkey`. */
const get = async (did: string, rkey: string, cid: string, query = '') =>
	callRoute(GET, {
		app: f.t,
		viewer: await f.t.viewer(did),
		url: `/dashboard/projects/${f.project.slug}/press-kits/${rkey}/photos/${cid}${query}`,
		params: { slug: f.project.slug, rkey, cid }
	});

/** The bytes a response carries, as text. */
const body = async (response: Response) => new TextDecoder().decode(await response.arrayBuffer());

it("serves a photo from the manager's draft, privately", async () => {
	const photo = await f.photo(DJ, 'Backstage at the warehouse');
	const draft = await f.t.epks.drafts.save(await f.t.viewer(DJ), f.project, { values: values({ photos: [photo] }) });

	const response = await get(DJ, draft!.rkey, photo.image.ref.$link);

	expect(response.headers.get('cache-control')).toBe('private, max-age=3600');
	expect(await body(response)).toBe('Backstage at the warehouse');
});

it('serves a photo the published press kit shows only to people with access, as a download when asked', async () => {
	const photo = await f.photo(DJ, 'Soundcheck');
	const epk = await f.publish(DJ, { content: { ...values().content, photos: [photo] } });
	// A draft of the other fields doesn't hide the published photos.
	await f.t.epks.drafts.save(await f.t.viewer(CODJ), f.project, {
		rkey: epk.rkey,
		cid: epk.cid,
		values: values({ headline: 'Changed', content: { ...values().content, photos: [photo] } })
	});

	const response = await get(CODJ, epk.rkey, photo.image.ref.$link, '?download');

	expect(response.headers.get('content-disposition')).toBe('attachment');
	expect(await body(response)).toBe('Soundcheck');
});

it('answers a photo neither the draft nor the press kit holds with a 404', async () => {
	const draft = await f.t.epks.drafts.save(await f.t.viewer(DJ), f.project, { values: values() });
	const stray = await f.photo(DJ, 'Not in the press kit');

	await expect(get(DJ, draft!.rkey, stray.image.ref.$link)).rejects.toMatchObject({ status: 404 });
});

it("refuses someone who doesn't manage the project", async () => {
	const photo = await f.photo(DJ, 'Soundcheck');
	const epk = await f.publish(DJ, { photos: [photo] });

	await expect(get(STRANGER, epk.rkey, photo.image.ref.$link)).rejects.toMatchObject({ status: 403 });
});
