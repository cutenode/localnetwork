import { afterEach, beforeEach, expect, it } from 'vitest';
import { sampleReport } from '#lib/mix/sample.ts';
import { COLLECTION } from '#lib/nsid.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { IndexedRepo } from '#lib/server/happyview/indexed.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { VerificationService } from '#lib/server/services/verifications.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute, type RouteEventInit } from '#lib/testing/route-event.ts';
import type { ProjectProfile, RequestView } from '#lib/types.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

const MC = 'did:plc:mc';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, DJ, MC] });
	await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** A solo techno project in Berlin called `name`. */
const profile = (name: string, overrides: Partial<ProjectProfile> = {}): ProjectProfile => ({
	name,
	kind: 'solo',
	genres: ['techno'],
	location: { city: 'Berlin', country: 'DE' },
	...overrides
});

const png = { bytes: new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1, 2, 3]), mimeType: 'image/png' };

/** The request's page as `did` sees it, with `init` on top. */
const as = async (did: string, request: RequestView, init: RouteEventInit = {}): Promise<RouteEventInit> => ({
	app: t,
	viewer: await t.viewer(did),
	url: `/admin/requests/${request.rkey}`,
	params: { rkey: request.rkey },
	...init
});

/** `did`'s claim on a new project of the admin's, with `relationship` to the act. */
async function claimAs(did: string, relationship: string) {
	const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile(`Act of ${did}`) });

	return t.requests.submit({
		actor: await t.viewer(did),
		kind: 'claim',
		projectUri: project.uri,
		claim: { relationship, evidence: 'Same handle on the flyer' }
	});
}

/** Approves `request` as the moderator. */
const approve = async (request: RequestView) =>
	t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' });

/** The DJ's profile with the DJ section on, saved over whatever they had. */
async function saveDjProfile(name: string) {
	const current = await t.profiles.get(DJ);

	await t.profiles.save(await t.viewer(DJ), {
		basics: { name, location: { city: 'Berlin', country: 'DE' } },
		performer: { genres: ['techno'] },
		avatar: 'keep',
		swap: current?.cid ?? null
	});

	return (await t.profiles.get(DJ))!;
}

/** The DJ's request to be listed in Find DJs. */
async function askToList() {
	const current = await saveDjProfile('Neon');

	return t.requests.submit({
		actor: await t.viewer(DJ),
		kind: 'list',
		listing: { subject: current.uri, cid: current.cid },
		message: 'Please list me'
	});
}

it('shows a new-project request with its proposed avatar, its tracklists, and what else the requester asked', async () => {
	const dj = await t.viewer(DJ);
	const other = await t.requests.submit({ actor: dj, kind: 'create', profile: profile('Deep Current') });

	const request = await t.requests.submit({
		actor: dj,
		kind: 'create',
		profile: profile('Neon Tide', {
			mixes: [
				{ title: 'Warehouse', url: 'https://mixes.test/warehouse' },
				{ title: 'Rooftop', url: 'https://mixes.test/rooftop' }
			]
		}),
		avatar: png,
		tracklists: [{ bytes: new TextEncoder().encode(JSON.stringify(sampleReport)), mimeType: 'application/json' }],
		message: 'First project!'
	});

	const data = await loadRoute<PageServerData>(
		load,
		await as(MOD, request, { url: `/admin/requests/${request.rkey}?decided=rejected` })
	);

	expect(data.request.uri).toBe(request.uri);
	expect(data).toMatchObject({ project: null, listing: null, managers: [], verification: null, stale: false });
	expect(data.changes.map((change) => change.field)).toContain('profile.name');
	expect(data.decided).toBe('Rejected');
	expect(data.history.map((item) => item.uri)).toEqual([other.uri]);
	expect(data.trail.map((entry) => entry.action)).toEqual(['request.submit']);
	expect(data.avatar).toEqual({ type: 'image/png', shown: true });
	expect(data.tracklists).toEqual([{ n: 1, title: 'Warehouse' }]);
});

it("flags an avatar the site won't show, and shows a request without one or any mixes", async () => {
	const dj = await t.viewer(DJ);

	const svg = {
		bytes: new TextEncoder().encode('<svg xmlns="http://www.w3.org/2000/svg"/>'),
		mimeType: 'image/svg+xml'
	};

	const vector = await t.requests.submit({ actor: dj, kind: 'create', profile: profile('Vector'), avatar: svg });
	const plain = await t.requests.submit({ actor: dj, kind: 'create', profile: profile('Plain') });

	expect((await loadRoute<PageServerData>(load, await as(MOD, vector))).avatar).toEqual({
		type: 'image/svg+xml',
		shown: false
	});
	expect(await loadRoute<PageServerData>(load, await as(MOD, plain))).toMatchObject({
		avatar: null,
		tracklists: [],
		decided: null
	});
});

it("shows an admin whether the account behind an approved claim on its own act, or a member's, is verified", async () => {
	const self = await claimAs(DJ, 'self');
	const member = await claimAs(MC, 'member');
	await approve(self);
	await approve(member);

	const data = await loadRoute<PageServerData>(load, await as(ADMIN, self));

	expect(data.project?.uri).toBe(self.projectUri);
	expect(data.managers).toEqual([{ did: DJ, handle: null }]);
	expect(await data.verification).toBe('unverified');
	expect(await (await loadRoute<PageServerData>(load, await as(ADMIN, member))).verification).toBe('unverified');
});

it("offers no verification to a moderator, for a manager's claim, or for a claim still pending", async () => {
	const self = await claimAs(DJ, 'self');
	const manager = await claimAs(MC, 'manager');
	await approve(self);
	await approve(manager);
	const pending = await claimAs(MOD, 'self');

	expect((await loadRoute<PageServerData>(load, await as(MOD, self))).verification).toBeNull();
	expect((await loadRoute<PageServerData>(load, await as(ADMIN, manager))).verification).toBeNull();
	expect((await loadRoute<PageServerData>(load, await as(ADMIN, pending))).verification).toBeNull();
});

it('reads no verification while HappyView is down, rather than calling the account unverified', async () => {
	const self = await claimAs(DJ, 'self');
	await approve(self);

	const down = new VerificationService(
		t.core,
		new IndexedRepo(
			new HappyViewClient({ url: 'http://happyview.test', fetch: async () => new Response('down', { status: 503 }) }),
			silentLogger,
			0
		),
		t.projects,
		{ resolve: async () => ({ handle: null }) }
	);

	const data = await loadRoute<PageServerData>(load, await as(ADMIN, self, { app: { ...t, verifications: down } }));

	expect(await data.verification).toBeNull();
});

it('shows staff a request to be listed with the profile as it is now and any solo act the DJ already manages', async () => {
	const admin = await t.viewer(ADMIN);
	const request = await askToList();
	const solo = await t.projectAdmin.create({ actor: admin, profile: profile('Neon Solo'), managers: [DJ] });
	await t.projectAdmin.create({ actor: admin, profile: profile('Neon Band', { kind: 'band' }), managers: [DJ] });
	const hidden = await t.projectAdmin.create({ actor: admin, profile: profile('Old Alias'), managers: [DJ] });
	await t.projectAdmin.setStatus({ actor: admin, project: hidden, status: 'hidden' });

	const data = await loadRoute<PageServerData>(load, await as(MOD, request));

	expect(data.listing).toMatchObject({
		profile: { did: DJ },
		changed: false,
		soloProjects: [{ slug: solo.slug, name: 'Neon Solo' }],
		listed: false
	});

	await saveDjProfile('Neon (live)');
	await approve(request);

	expect((await loadRoute<PageServerData>(load, await as(MOD, request))).listing).toMatchObject({
		changed: true,
		listed: true
	});
});

it('shows a request to be listed from a DJ whose profile is gone', async () => {
	const request = await askToList();
	await t.net.client(DJ).deleteRecord({ collection: COLLECTION.profile, rkey: 'self' });

	expect((await loadRoute<PageServerData>(load, await as(MOD, request))).listing).toMatchObject({
		profile: null,
		changed: false
	});
});

it("answers 404 for a request that isn't there, on the page and when deciding", async () => {
	const init = { app: t, url: '/admin/requests/3mnothere', params: { rkey: '3mnothere' } };

	await expect(loadRoute<PageServerData>(load, { ...init, viewer: await t.viewer(MOD) })).rejects.toMatchObject({
		status: 404
	});
	await expect(
		callRoute(actions.decide, { ...init, viewer: await t.viewer(MOD), form: { outcome: 'approved' } })
	).rejects.toMatchObject({ status: 404, body: { message: 'Request not found' } });
});

it('asks for a decision when none was chosen', async () => {
	const request = await t.requests.submit({ actor: await t.viewer(DJ), kind: 'create', profile: profile('Neon Tide') });

	expect(await callRoute(actions.decide, await as(MOD, request, { form: { outcome: 'maybe' } }))).toEqual({
		error: 'Choose a decision'
	});
	expect((await t.requests.get(request.uri))?.status).toBe('pending');
});

it("moves on to the oldest request waiting that the moderator didn't file themselves", async () => {
	const first = await t.requests.submit({ actor: await t.viewer(DJ), kind: 'create', profile: profile('Neon Tide') });
	await t.requests.submit({ actor: await t.viewer(MOD), kind: 'create', profile: profile('Mod Squad') });
	const next = await t.requests.submit({ actor: await t.viewer(MC), kind: 'create', profile: profile('Deep Current') });

	await expect(
		callRoute(actions.decide, await as(MOD, first, { form: { outcome: 'rejected', reason: 'Duplicate' } }))
	).rejects.toMatchObject({ status: 303, location: `/admin/requests/${next.rkey}?decided=rejected` });
	expect((await t.requests.get(first.uri))?.decision?.reason).toBe('Duplicate');

	await expect(callRoute(actions.decide, await as(MOD, next, { form: { outcome: 'approved' } }))).rejects.toMatchObject(
		{ status: 303, location: '/admin/requests?decided=approved' }
	);
});

it('says a DJ was listed once their request to be listed is approved', async () => {
	const request = await askToList();

	await expect(
		callRoute(actions.decide, await as(MOD, request, { form: { outcome: 'approved' } }))
	).rejects.toMatchObject({ status: 303, location: '/admin/requests?decided=listed' });
	expect(await t.djListings.get(DJ)).not.toBeNull();
});

it('keeps the moderator on the request when the decision is refused', async () => {
	const request = await t.requests.submit({ actor: await t.viewer(DJ), kind: 'create', profile: profile('Neon Tide') });
	await approve(request);

	const form = { outcome: 'rejected', reason: 'Changed my mind' };

	expect(await callRoute(actions.decide, await as(MOD, request, { form }))).toMatchObject({
		status: 400,
		data: { error: 'This request is not waiting for review' }
	});
	expect((await t.requests.get(request.uri))?.status).toBe('approved');
});
