import { afterEach, beforeEach, expect, it } from 'vitest';
import { BSKY_PROFILE } from '#lib/nsid.ts';
import { SCOPES } from '#lib/server/atproto/oauth.ts';
import { createTestServices, ROOT } from '#lib/server/testing/core.ts';
import type { Viewer } from '#lib/types.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const DJ = 'did:plc:dj';

/** An account whose PDS session the app doesn't hold, so it can't read their Bluesky profile or write for them. */
const LAPSED = 'did:plc:lapsed';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, DJ] });
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The app as the profile pages read it, with the geocoder off. */
const app = () => ({ ...t, rootDid: ROOT, geocoder: { enabled: false }, sets: t.projects.sets });

/** DJ's Bluesky profile, as their own repo holds it, with a picture when `avatar` is set. */
async function blueskyProfile(
	did: string,
	fields: { displayName?: string; description?: string; pronouns?: string },
	avatar = false
) {
	const client = t.net.client(did);
	const image = avatar ? await client.uploadBlob(new TextEncoder().encode('portrait'), 'image/png') : undefined;

	await client.putRecord({
		collection: BSKY_PROFILE,
		rkey: 'self',
		record: { $type: BSKY_PROFILE, ...fields, ...(image && { avatar: image }) }
	});

	return image;
}

/** The profile editor's load for `did`. */
const loaded = async (did: string) =>
	loadRoute<PageServerData>(load, { app: app(), viewer: await t.viewer(did), url: '/dashboard/profile' });

/** Saves DJ's profile with the performer section on. */
async function savedProfile() {
	await t.profiles.save(await t.viewer(DJ), {
		basics: { name: 'Neon', bio: 'Rolling techno' },
		performer: { genres: ['techno'] },
		avatar: 'remove',
		swap: null
	});

	return (await t.profiles.get(DJ))!;
}

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: app(), url: '/dashboard/profile' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Fprofile'
	});
});

it('starts someone without a profile from their Bluesky one, and asks a session from before profiles to sign in again', async () => {
	await blueskyProfile(DJ, { displayName: 'Neon on Bluesky', description: 'Berlin techno' });

	const data = await loaded(DJ);

	expect(data).toMatchObject({
		profile: null,
		values: { name: 'Neon on Bluesky', bio: 'Berlin techno' },
		sectionsOn: { performer: false, promoter: false },
		listing: null,
		listRequest: null,
		needsSignIn: true,
		geocoder: false
	});
	expect(await data.sets).toEqual([]);
});

it('opens with an empty form when the app has no Bluesky profile to start from', async () => {
	const data = await loaded(LAPSED);

	expect(data.values).toEqual({ name: undefined, bio: undefined });
});

it('fills the form in from a saved profile, with the request to be listed that is waiting', async () => {
	const profile = await savedProfile();
	await t.accounts.recordLogin({
		did: DJ,
		handle: 'neon.test',
		pdsUrl: 'https://pds.test',
		grantedScope: SCOPES.base,
		spacesCapable: false,
		publicWrites: true
	});

	const request = await t.requests.submit({
		actor: await t.viewer(DJ),
		kind: 'list',
		listing: { subject: profile.uri, cid: profile.cid }
	});

	const data = await loaded(DJ);

	expect(data).toMatchObject({
		profile: { uri: profile.uri },
		values: { name: 'Neon', bio: 'Rolling techno', genres: 'techno' },
		sectionsOn: { performer: true, promoter: false },
		listing: null,
		listRequest: { rkey: request.rkey, status: 'pending' },
		needsSignIn: false
	});
});

it("shows the pronouns from someone's Bluesky profile, else the ones their profile kept, else none", async () => {
	await t.profiles.save(await t.viewer(DJ), {
		basics: { name: 'Neon', pronouns: 'they/them' },
		avatar: 'remove',
		swap: null
	});
	expect((await loaded(DJ)).pronouns).toBe('they/them');

	await blueskyProfile(DJ, { displayName: 'Neon', pronouns: 'she/her' });
	expect((await loaded(DJ)).pronouns).toBe('she/her');
	expect((await loaded(LAPSED)).pronouns).toBe('');
});

it('says when staff list the account in Find DJs', async () => {
	await savedProfile();
	const listed = await t.djListings.list(await t.viewer(ADMIN), DJ);

	const data = await loaded(DJ);

	expect(data.listing).toEqual({ listedAt: listed.createdAt });
});

it('saves a new profile, then changes it from the version the form opened on', async () => {
	const viewer = await t.viewer(DJ);
	const form = { name: 'Neon', bio: 'Rolling techno', section: 'performer', genres: 'techno, house' };

	await expect(
		callRoute(actions.save, { app: app(), viewer, url: '/dashboard/profile?/save', form })
	).rejects.toMatchObject({ status: 303, location: '/dashboard/profile?saved=1' });

	const saved = (await t.profiles.get(DJ))!;
	expect(saved.record).toMatchObject({ basics: { name: 'Neon' }, performer: { genres: ['techno', 'house'] } });

	await expect(
		callRoute(actions.save, {
			app: app(),
			viewer,
			url: '/dashboard/profile?/save',
			form: { ...form, name: 'Neon Tide', cid: saved.cid }
		})
	).rejects.toMatchObject({ status: 303 });

	expect((await t.profiles.get(DJ))?.record.basics.name).toBe('Neon Tide');
});

it('keeps what was typed and says what to fix when the profile has no name', async () => {
	const result = await callRoute(actions.save, {
		app: app(),
		viewer: await t.viewer(DJ),
		url: '/dashboard/profile?/save',
		form: { bio: 'Rolling techno' }
	});

	expect(result).toMatchObject({ status: 400, data: { error: 'Add your name', values: { bio: 'Rolling techno' } } });
	expect(await t.profiles.get(DJ)).toBeNull();
});

it('asks staff to list the profile in Find DJs, and opens the request', async () => {
	const profile = await savedProfile();

	const sent = callRoute(actions.requestListing, {
		app: app(),
		viewer: await t.viewer(DJ),
		url: '/dashboard/profile?/requestListing',
		form: { message: 'I play every weekend' }
	});

	await expect(sent).rejects.toMatchObject({ status: 303 });
	const [request] = await t.requests.list({ kind: 'list', requester: DJ });
	await expect(sent).rejects.toMatchObject({ location: `/dashboard/requests/${request.rkey}?submitted=1` });
	expect(request?.record).toMatchObject({
		listing: { subject: profile.uri, cid: profile.cid },
		message: 'I play every weekend'
	});
});

it('refuses to ask for a listing before there is a profile to list', async () => {
	const result = await callRoute(actions.requestListing, {
		app: app(),
		viewer: await t.viewer(DJ),
		url: '/dashboard/profile?/requestListing',
		form: {}
	});

	expect(result).toMatchObject({ status: 400, data: { error: 'Set up your profile first' } });
	expect(await t.requests.list({ requester: DJ })).toEqual([]);
});

it('creates a profile from the first-run card with the Bluesky picture and bio', async () => {
	const avatar = await blueskyProfile(DJ, { displayName: 'Neon on Bluesky', description: 'Berlin techno' }, true);

	await expect(
		callRoute(actions.create, {
			app: app(),
			viewer: await t.viewer(DJ),
			url: '/dashboard/profile?/create',
			form: { name: 'Neon', blueskyAvatar: 'on' }
		})
	).rejects.toMatchObject({ status: 303, location: '/dashboard/profile?created=1' });

	expect((await t.profiles.get(DJ))?.record.basics).toEqual({ name: 'Neon', bio: 'Berlin techno', avatar });
});

it("names a profile from the card after the Bluesky name, and leaves the picture off when it isn't asked for", async () => {
	await blueskyProfile(DJ, { displayName: 'Neon on Bluesky' }, true);

	await expect(
		callRoute(actions.create, {
			app: app(),
			viewer: await t.viewer(DJ),
			url: '/dashboard/profile?/create',
			form: {}
		})
	).rejects.toMatchObject({ status: 303 });

	expect((await t.profiles.get(DJ))?.record.basics).toEqual({ name: 'Neon on Bluesky' });
});

it('names a profile from the card after the handle, or the DID, without a Bluesky profile', async () => {
	const viewer = { ...(await t.viewer(DJ)), handle: 'neon.test' };

	const create = (as: Viewer) =>
		callRoute(actions.create, {
			app: app(),
			viewer: as,
			url: '/dashboard/profile?/create',
			form: { blueskyAvatar: 'on' }
		});

	await expect(create(viewer)).rejects.toMatchObject({ status: 303 });
	expect((await t.profiles.get(DJ))?.record.basics).toEqual({ name: 'neon.test' });

	await expect(create(await t.viewer(ADMIN))).rejects.toMatchObject({ status: 303 });
	expect((await t.profiles.get(ADMIN))?.record.basics).toEqual({ name: ADMIN });
});

it('refuses a name from the card that is too long', async () => {
	const result = await callRoute(actions.create, {
		app: app(),
		viewer: await t.viewer(DJ),
		url: '/dashboard/profile?/create',
		form: { name: 'N'.repeat(65) }
	});

	expect(result).toMatchObject({ status: 400, data: { error: 'Name can be at most 64 characters' } });
	expect(await t.profiles.get(DJ)).toBeNull();
});
