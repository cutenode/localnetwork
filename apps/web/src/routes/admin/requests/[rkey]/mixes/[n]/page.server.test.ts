import { afterEach, beforeEach, expect, it } from 'vitest';
import { sampleReport } from '#lib/mix/sample.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { loadRoute, type RouteEventInit } from '#lib/testing/route-event.ts';
import type { ProjectProfile, RequestView } from '#lib/types.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

let request: RequestView;

const profile: ProjectProfile = {
	name: 'Neon Tide',
	kind: 'solo',
	genres: ['techno'],
	location: { city: 'Berlin', country: 'DE' },
	mixes: [
		{ title: 'Warehouse', url: 'https://mixes.test/warehouse' },
		{ title: 'Rooftop', url: 'https://mixes.test/rooftop' }
	]
};

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, DJ] });
	await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');

	request = await t.requests.submit({
		actor: await t.viewer(DJ),
		kind: 'create',
		profile,
		tracklists: [{ bytes: new TextEncoder().encode(JSON.stringify(sampleReport)), mimeType: 'application/json' }]
	});
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The tracklist page for mix `n` of `rkey`, as the moderator sees it, with `init` on top. */
const mix = async (n: string, rkey = request.rkey, init: RouteEventInit = {}) =>
	loadRoute<PageServerData>(load, {
		app: t,
		viewer: await t.viewer(MOD),
		url: `/admin/requests/${rkey}/mixes/${n}`,
		params: { rkey, n },
		...init
	});

it('shows reviewers the tracklist proposed for a mix', async () => {
	const data = await mix('1');

	expect(data.rkey).toBe(request.rkey);
	expect(data.mix).toEqual({ title: 'Warehouse', url: 'https://mixes.test/warehouse' });
	expect(data.report.mix.title).toBe(sampleReport.mix.title);
	expect(data.report.plays).toHaveLength(sampleReport.plays.length);
});

it('answers 404 for a mix without a tracklist, past the last mix, or not a mix number', async () => {
	for (const n of ['2', '3', '0', '1.5', 'first'])
		await expect(mix(n)).rejects.toMatchObject({ status: 404, body: { message: 'This mix has no tracklist' } });
});

it('answers 404 for a request without a profile, or no request at all', async () => {
	const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: { ...profile, mixes: [] } });

	const claim = await t.requests.submit({
		actor: await t.viewer(DJ),
		kind: 'claim',
		projectUri: project.uri,
		claim: { relationship: 'self', evidence: 'Same handle on the flyer' }
	});

	await expect(mix('1', claim.rkey)).rejects.toMatchObject({
		status: 404,
		body: { message: 'This mix has no tracklist' }
	});
	await expect(mix('1', '3mnothere')).rejects.toMatchObject({ status: 404, body: { message: 'Request not found' } });
});

it("says so when the tracklist can't be fetched from the requester or the root", async () => {
	t.net.blobs.delete(request.record.profile!.mixes![0].tracklist!.ref.$link);

	await expect(mix('1')).rejects.toMatchObject({
		status: 502,
		body: { message: 'This tracklist could not be fetched right now.' }
	});
});

it('lets an unexpected failure through as it is', async () => {
	const broken = new TypeError('fetch failed');
	const app = { ...t, tracklists: { proposed: async () => Promise.reject(broken) } };

	await expect(mix('1', request.rkey, { app })).rejects.toBe(broken);
});
