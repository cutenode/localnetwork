import { afterEach, beforeEach, expect, it } from 'vitest';
import { ADMIN, DJ, pressKitServices, STRANGER } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0]);

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The app with the geocoder off and the managers' sets. */
const app = () => ({ ...f.t, geocoder: { enabled: false }, sets: f.t.projects.sets });

/** The project's profile form, filled in as it's published. */
const form = { name: 'Neon Tide', kind: 'solo', genres: 'techno', city: 'Berlin', country: 'DE' };

/** The edit form's load for DJ, under the layout that found the project. */
const loaded = async () =>
	loadRoute<PageServerData>(load, {
		app: app(),
		viewer: await f.t.viewer(DJ),
		url: `/dashboard/projects/${f.project.slug}/profile`,
		params: { slug: f.project.slug },
		parent: { project: f.project }
	});

/** What `did` sends from the edit form: the failure it comes back with, or the redirect it rejects with. */
const send = async (did: string, fields: Record<string, string | Blob>) =>
	callRoute(actions.default, {
		app: app(),
		viewer: await f.t.viewer(did),
		url: `/dashboard/projects/${f.project.slug}/profile`,
		params: { slug: f.project.slug },
		form: fields
	});

/** DJ's open change request for the project. */
const open = async () => f.t.requests.openUpdate(f.project.uri, DJ);

/** Gives the project a published photo. */
async function withAvatar() {
	f.project = await f.t.projectAdmin.update({
		actor: await f.t.viewer(ADMIN),
		project: f.project,
		profile: f.project.profile,
		details: { bookingEmail: 'book@neon.test' },
		avatar: { bytes: PNG, mimeType: 'image/png' }
	});

	return f.project.profile.avatar!;
}

it('opens the form on the published profile and its private details', async () => {
	await withAvatar();

	const data = await loaded();

	expect(data).toMatchObject({
		values: { name: 'Neon Tide', genres: 'techno', city: 'Berlin', country: 'DE', bookingEmail: 'book@neon.test' },
		geocoder: false
	});
	expect(await data.sets).toEqual([]);
});

it('goes back to the change request that is already open instead of starting over', async () => {
	await expect(send(DJ, { ...form, name: 'Neon Tide Live' })).rejects.toMatchObject({ status: 303 });
	const request = (await open())!;

	await expect(loaded()).rejects.toMatchObject({
		status: 303,
		location: `/dashboard/account/requests/${request.rkey}`
	});
});

it('proposes changes for review, keeping the published photo', async () => {
	const avatar = await withAvatar();

	const sent = send(DJ, { ...form, name: 'Neon Tide Live', message: 'We renamed the live act' });

	await expect(sent).rejects.toMatchObject({ status: 303 });
	const request = (await open())!;
	await expect(sent).rejects.toMatchObject({ location: `/dashboard/account/requests/${request.rkey}?submitted=1` });
	expect(request.record).toMatchObject({
		kind: 'update',
		profile: { name: 'Neon Tide Live', avatar: { ref: { $link: avatar.ref.$link } } },
		message: 'We renamed the live act'
	});
});

it('proposes taking the photo off when asked', async () => {
	await withAvatar();

	await expect(send(DJ, { ...form, removeAvatar: 'on' })).rejects.toMatchObject({ status: 303 });

	expect((await open())?.record.profile?.avatar).toBeUndefined();
});

it('proposes a new photo for a project without one', async () => {
	await expect(send(DJ, { ...form, avatar: new File([PNG], 'neon.png', { type: 'image/png' }) })).rejects.toMatchObject(
		{ status: 303 }
	);

	expect((await open())?.record.profile?.avatar).toMatchObject({ mimeType: 'image/png', size: PNG.length });
});

it('keeps what was typed and says what to fix when the profile has no genre', async () => {
	const result = await send(DJ, { ...form, genres: '' });

	expect(result).toMatchObject({
		status: 400,
		data: { error: 'Add at least one genre', values: { name: 'Neon Tide' } }
	});
	expect(await open()).toBeNull();
});

it("refuses changes from someone who doesn't manage the project", async () => {
	await expect(send(STRANGER, form)).rejects.toMatchObject({
		status: 403,
		body: { message: "Only this project's managers can manage it" }
	});
	expect(await f.t.requests.list({ requester: STRANGER })).toEqual([]);
});
