import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { RequestView } from '#lib/types.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

let pending: RequestView;

let rejected: RequestView;

let claim: RequestView;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, DJ] });
	const admin = await t.viewer(ADMIN);
	const dj = await t.viewer(DJ);
	await t.staff.grant(admin, MOD, 'moderator');

	const profile = (name: string) => ({
		name,
		kind: 'solo' as const,
		genres: ['techno'],
		location: { city: 'Berlin', country: 'DE' }
	});

	pending = await t.requests.submit({ actor: dj, kind: 'create', profile: profile('Neon Tide') });
	rejected = await t.requests.submit({ actor: dj, kind: 'create', profile: profile('Deep Current') });
	await t.requests.decide({
		actor: await t.viewer(MOD),
		uri: rejected.uri,
		outcome: 'rejected',
		reason: 'Already listed'
	});

	const project = await t.projectAdmin.create({ actor: admin, profile: profile('Iron Pulse') });

	claim = await t.requests.submit({
		actor: dj,
		kind: 'claim',
		projectUri: project.uri,
		claim: { relationship: 'self', evidence: 'It me' }
	});
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The queue as the moderator sees it at `url`. */
const queue = async (url: string) => loadRoute<PageServerData>(load, { app: t, viewer: await t.viewer(MOD), url });

/** The URIs on a page of the queue. */
const uris = (data: Awaited<ReturnType<typeof queue>>) => data.requests.map((request) => request.uri).sort();

it('opens on the pending requests, with counts for every status', async () => {
	const data = await queue('/admin/requests');

	expect(uris(data)).toEqual([pending.uri, claim.uri].sort());
	expect(data.counts).toMatchObject({ pending: 2, rejected: 1 });
	expect(data).toMatchObject({ status: 'pending', kind: '', decided: null });
});

it('shows every status, one status, or one kind', async () => {
	expect(uris(await queue('/admin/requests?status=all'))).toEqual([pending.uri, rejected.uri, claim.uri].sort());
	expect(uris(await queue('/admin/requests?status=rejected'))).toEqual([rejected.uri]);

	const claims = await queue('/admin/requests?kind=claim');
	expect(uris(claims)).toEqual([claim.uri]);
	expect(claims.kind).toBe('claim');
});

it("falls back to pending for a status it doesn't know, and to every kind for a kind it doesn't know", async () => {
	const data = await queue('/admin/requests?status=lost&kind=refund');

	expect(uris(data)).toEqual([pending.uri, claim.uri].sort());
	expect(data).toMatchObject({ status: 'pending', kind: '' });
});

it('says what the last decision did', async () => {
	expect((await queue('/admin/requests?decided=rejected')).decided).toBe('Rejected');
	expect((await queue('/admin/requests?decided=exploded')).decided).toBeNull();
});

it('sends someone signed out to sign in', async () => {
	await expect(loadRoute<PageServerData>(load, { app: t, url: '/admin/requests' })).rejects.toMatchObject({
		status: 303
	});
});
