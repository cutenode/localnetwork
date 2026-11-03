import { afterEach, beforeEach, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { createTestServices, ROOT } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, DJ] });
	await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The DJ saves a profile with the DJ section on and asks to be listed; the moderator approves. */
async function listDj() {
	await t.profiles.save(await t.viewer(DJ), {
		basics: { name: 'Neon', location: { city: 'Berlin', country: 'DE' } },
		performer: { genres: ['techno'] },
		avatar: 'keep',
		swap: null
	});

	const profile = (await t.profiles.get(DJ))!;

	const request = await t.requests.submit({
		actor: await t.viewer(DJ),
		kind: 'list',
		listing: { subject: profile.uri, cid: profile.cid },
		message: 'Please list me'
	});

	await t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' });

	return request;
}

it("shows staff a listed DJ's account, profile, listing, and what they asked", async () => {
	const request = await listDj();

	const data = await loadRoute<PageServerData>(load, {
		app: t,
		viewer: await t.viewer(MOD),
		url: `/admin/listings/${DJ}`,
		params: { did: DJ }
	});

	expect(data.account).toEqual({ did: DJ, handle: null });
	expect(data.profile?.record.basics.name).toBe('Neon');
	expect(data.listing).toMatchObject({ uri: `at://${ROOT}/${COLLECTION.listingProfile}/${DJ}`, request: request.uri });
	expect(data.requests.map((item) => [item.uri, item.status])).toEqual([[request.uri, 'approved']]);
});

it("shows an account that never asked as neither listed nor with a profile, and 404s what isn't a DID", async () => {
	const viewer = await t.viewer(MOD);

	const data = await loadRoute<PageServerData>(load, {
		app: t,
		viewer,
		url: `/admin/listings/${DJ}`,
		params: { did: DJ }
	});

	expect(data).toMatchObject({ profile: null, listing: null, requests: [] });
	await expect(
		loadRoute<PageServerData>(load, { app: t, viewer, url: '/admin/listings/neon.test', params: { did: 'neon.test' } })
	).rejects.toMatchObject({ status: 404, body: { message: 'No account here' } });
});

it('takes a DJ out of Find DJs', async () => {
	await listDj();

	const result = await callRoute(actions.unlist, {
		app: t,
		viewer: await t.viewer(MOD),
		url: `/admin/listings/${DJ}?/unlist`,
		params: { did: DJ },
		form: {}
	});

	expect(result).toEqual({ success: 'Taken out of Find DJs' });
	expect(await t.djListings.get(DJ)).toBeNull();
});

it('refuses to unlist for someone signed out', async () => {
	await listDj();

	await expect(
		callRoute(actions.unlist, { app: t, url: `/admin/listings/${DJ}?/unlist`, params: { did: DJ }, form: {} })
	).rejects.toMatchObject({ status: 303 });
	expect(await t.djListings.get(DJ)).not.toBeNull();
});
