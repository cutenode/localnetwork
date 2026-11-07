import { afterEach, beforeEach, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { ROOT } from '#lib/server/testing/core.ts';
import { ADMIN, DJ, pressKitServices, STRANGER } from '#lib/server/testing/press-kits.ts';
import type { ProjectProfile } from '#lib/types.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0]);

/** Another PNG, so its blob has another CID. */
const OTHER_PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1]);

/** A new project as a request asks for it. */
const LOW_HUM: ProjectProfile = {
	name: 'Low Hum',
	kind: 'duo',
	genres: ['house'],
	location: { city: 'Leipzig', country: 'DE' }
};

/** The project form for Low Hum, filled in. */
const form = { name: 'Low Hum', kind: 'duo', genres: 'house', city: 'Leipzig', country: 'DE' };

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

/** The request page's load for `did` (signed out without one) at `rkey`, with `query`. */
const loaded = async (did: string | null, rkey: string, query = '') =>
	loadRoute<PageServerData>(load, {
		app: app(),
		viewer: did ? await f.t.viewer(did) : null,
		url: `/dashboard/requests/${rkey}${query}`,
		params: { rkey }
	});

/** What `did` sends with the `action` at the request `rkey`: what it returns, or the redirect it rejects with. */
const send = async (action: 'withdraw' | 'revise', did: string, rkey: string, fields: Record<string, string> = {}) =>
	callRoute(actions[action], {
		app: app(),
		viewer: await f.t.viewer(did),
		url: `/dashboard/requests/${rkey}?/${action}`,
		params: { rkey },
		form: fields
	});

/** DJ asks for the new project Low Hum, with a photo when `avatar` is set. */
const askCreate = async (avatar = false) =>
	f.t.requests.submit({
		actor: await f.t.viewer(DJ),
		kind: 'create',
		profile: LOW_HUM,
		avatar: avatar ? { bytes: PNG, mimeType: 'image/png' } : undefined
	});

/** DJ asks to rename the project, with a new photo when `avatar` is set. */
const askUpdate = async (avatar = false) =>
	f.t.requests.submit({
		actor: await f.t.viewer(DJ),
		kind: 'update',
		projectUri: f.project.uri,
		profile: { ...f.project.profile, name: 'Neon Tide Live' },
		avatar: avatar ? { bytes: OTHER_PNG, mimeType: 'image/png' } : undefined
	});

/** STRANGER's claim on the project. */
const askClaim = async () =>
	f.t.requests.submit({
		actor: await f.t.viewer(STRANGER),
		kind: 'claim',
		projectUri: f.project.uri,
		claim: { relationship: 'self', evidence: 'My Bluesky links here' },
		message: 'It is me'
	});

/** Gives the project a published photo. */
async function withAvatar() {
	f.project = await f.t.projectAdmin.update({
		actor: await f.t.viewer(ADMIN),
		project: f.project,
		profile: f.project.profile,
		avatar: { bytes: PNG, mimeType: 'image/png' }
	});
}

it('shows a new project request just sent, with its photo from the requester', async () => {
	const request = await askCreate(true);

	const data = await loaded(DJ, request.rkey, '?submitted=1');

	expect(data).toMatchObject({
		request: { uri: request.uri },
		project: null,
		changes: [],
		avatarUrl: `/dashboard/requests/${request.rkey}/avatar`,
		values: { name: 'Low Hum', genres: 'house', city: 'Leipzig' },
		submitted: true,
		geocoder: false,
		sets: undefined
	});
});

it('shows what a change request changes, with the published photo it kept', async () => {
	await withAvatar();
	const request = await askUpdate();

	const data = await loaded(DJ, request.rkey);

	expect(data).toMatchObject({
		project: { uri: f.project.uri },
		changes: [{ field: 'profile.name', before: 'Neon Tide', after: 'Neon Tide Live' }],
		avatarUrl: f.project.avatarUrl,
		submitted: false
	});
	expect(await data.sets).toEqual([]);
});

it('shows a new photo a change request proposes, and none when it has none', async () => {
	const proposed = await askUpdate(true);

	expect((await loaded(DJ, proposed.rkey)).avatarUrl).toBe(`/dashboard/requests/${proposed.rkey}/avatar`);

	await send('withdraw', DJ, proposed.rkey);
	const plain = await askUpdate();

	expect((await loaded(DJ, plain.rkey)).avatarUrl).toBeNull();
});

it('shows a change request for a project that is gone, with no changes and no set picker', async () => {
	const request = await askUpdate();
	await f.t.net.client(ROOT).deleteRecord({ collection: COLLECTION.project, rkey: f.project.uri.split('/').at(-1)! });

	expect(await loaded(DJ, request.rkey)).toMatchObject({ project: null, changes: [], sets: undefined });
});

it("shows staff anyone's request, and nobody else", async () => {
	const request = await askCreate();

	expect((await loaded(ADMIN, request.rkey)).request.uri).toBe(request.uri);
	await expect(loaded(STRANGER, request.rkey)).rejects.toMatchObject({ status: 404 });
	await expect(loaded(DJ, 'missing')).rejects.toMatchObject({ status: 404 });
	await expect(loaded(null, request.rkey)).rejects.toMatchObject({ status: 303 });
});

it('withdraws a request, once', async () => {
	const request = await askCreate();

	expect(await send('withdraw', DJ, request.rkey)).toEqual({ success: 'Request withdrawn' });
	expect((await f.t.requests.byRkey(request.rkey))?.record.withdrawnAt).toBeDefined();

	expect(await send('withdraw', DJ, request.rkey)).toMatchObject({
		status: 400,
		data: { error: 'This request has already been decided' }
	});
});

it('revises a new project request, keeping its photo', async () => {
	const request = await askCreate(true);
	const avatar = request.record.profile!.avatar!;

	await expect(
		send('revise', DJ, request.rkey, { ...form, name: 'Low Hum Live', message: 'New name' })
	).rejects.toMatchObject({
		status: 303,
		location: `/dashboard/requests/${request.rkey}`
	});

	expect((await f.t.requests.byRkey(request.rkey))?.record).toMatchObject({
		profile: { name: 'Low Hum Live', avatar: { ref: { $link: avatar.ref.$link } } },
		message: 'New name'
	});
});

it('revises a request to take its photo off, and one without a photo keeps none', async () => {
	const pictured = await askCreate(true);

	await expect(send('revise', DJ, pictured.rkey, { ...form, removeAvatar: 'on' })).rejects.toMatchObject({
		status: 303
	});

	expect((await f.t.requests.byRkey(pictured.rkey))?.record.profile?.avatar).toBeUndefined();

	const plain = await askUpdate();

	await expect(
		send('revise', DJ, plain.rkey, { name: 'Neon Tide', kind: 'solo', genres: 'techno', city: 'Berlin', country: 'DE' })
	).rejects.toMatchObject({ status: 303 });

	expect((await f.t.requests.byRkey(plain.rkey))?.record).toMatchObject({ profile: { name: 'Neon Tide' } });
	expect((await f.t.requests.byRkey(plain.rkey))?.record.profile?.avatar).toBeUndefined();
});

it('keeps what was typed and says what to fix in a revised profile', async () => {
	const request = await askCreate();

	expect(await send('revise', DJ, request.rkey, { ...form, genres: '' })).toMatchObject({
		status: 400,
		data: { error: 'Add at least one genre', values: { name: 'Low Hum' } }
	});
});

it("revises a claim's evidence, and clears its message when left blank", async () => {
	const request = await askClaim();

	await expect(
		send('revise', STRANGER, request.rkey, {
			relationship: 'member',
			evidence: 'Our label page lists me',
			message: 'Still me'
		})
	).rejects.toMatchObject({ status: 303 });

	expect((await f.t.requests.byRkey(request.rkey))?.record).toMatchObject({
		claim: { relationship: 'member', evidence: 'Our label page lists me' },
		message: 'Still me'
	});

	await expect(
		send('revise', STRANGER, request.rkey, { relationship: 'member', evidence: 'Our label page lists me' })
	).rejects.toMatchObject({ status: 303 });

	expect((await f.t.requests.byRkey(request.rkey))?.record.message).toBeUndefined();
});

it('keeps what was typed and says what to fix in a revised claim', async () => {
	const request = await askClaim();

	expect(await send('revise', STRANGER, request.rkey, { relationship: 'fan', evidence: 'Trust me' })).toMatchObject({
		status: 400,
		data: {
			error: 'Choose how you are connected to the project',
			values: { relationship: 'fan', evidence: 'Trust me' }
		}
	});
});

it('sends a listing request sent back for changes to staff again, with a note and no profile form', async () => {
	await f.t.profiles.save(await f.t.viewer(DJ), {
		basics: { name: 'Neon' },
		performer: {},
		avatar: 'keep',
		swap: null
	});
	const profile = (await f.t.profiles.get(DJ))!;

	const request = await f.t.requests.submit({
		actor: await f.t.viewer(DJ),
		kind: 'list',
		listing: { subject: profile.uri, cid: profile.cid }
	});

	await f.t.requests.decide({
		actor: await f.t.viewer(ADMIN),
		uri: request.uri,
		outcome: 'changesRequested',
		reason: 'Add a photo'
	});

	await expect(send('revise', DJ, request.rkey, { message: 'Added one' })).rejects.toMatchObject({ status: 303 });

	expect(await f.t.requests.byRkey(request.rkey)).toMatchObject({
		status: 'pending',
		record: { kind: 'list', message: 'Added one' }
	});
});
