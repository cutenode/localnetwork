import { AtUri } from '@atproto/syntax';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { DjActivity } from '#lib/dj.ts';
import type { PageMeta } from '#lib/meta.ts';
import type { ProjectProfile, ProjectView } from '#lib/types.ts';
import { performerMeta } from './meta';
import { djProfiles, type PageRecords, profileRecords, projectRecords, withRecords } from './meta-records';
import { createTestServices } from './testing/core';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

const CLAIMER = 'did:plc:claimer';

/** Named a manager by an admin, never asked. */
const NAMED = 'did:plc:named';

const profile = (name: string): ProjectProfile => ({
	name,
	kind: 'solo',
	genres: ['house'],
	location: { city: 'Berlin', country: 'DE' }
});

/** Every value parses as an AT URI whose repo is a DID. */
const expectDidUris = (records: PageRecords) => {
	for (const uri of [...records.canonical, ...records.alternate, ...records.author])
		expect(new AtUri(uri).host, uri).toMatch(/^did:[a-z]+:/);
};

describe('projectRecords', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, DJ, CLAIMER] });
		await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	it('credits only the managers who asked for the project, never one an admin named', async () => {
		const mod = await t.viewer(MOD);
		const created = await t.requests.submit({ actor: await t.viewer(DJ), kind: 'create', profile: profile('Glow') });
		await t.requests.decide({ actor: mod, uri: created.uri, outcome: 'approved' });
		const glow = (await t.projects.bySlug('glow'))!;

		const claim = await t.requests.submit({
			actor: await t.viewer(CLAIMER),
			kind: 'claim',
			projectUri: glow.uri,
			claim: { relationship: 'self', evidence: 'my site' }
		});

		await t.requests.decide({ actor: mod, uri: claim.uri, outcome: 'approved' });
		const claimed = (await t.projects.bySlug('glow'))!;

		const project = await t.projectAdmin.setManagers({
			actor: await t.viewer(ADMIN),
			project: claimed,
			managers: [...claimed.managers, NAMED]
		});

		expect(project.managers).toEqual([DJ, CLAIMER, NAMED]);
		const records = await projectRecords(t.requests, project);

		expect(records).toEqual({ canonical: [project.uri], alternate: [], author: [`at://${DJ}`, `at://${CLAIMER}`] });
		expectDidUris(records);
	});

	it('credits nobody on a project an admin added, managers or not', async () => {
		const admin = await t.viewer(ADMIN);
		const named = await t.projectAdmin.create({ actor: admin, profile: profile('Named'), managers: [NAMED, DJ] });
		const unclaimed = await t.projectAdmin.create({ actor: admin, profile: profile('Unclaimed') });

		expect((await projectRecords(t.requests, named)).author).toEqual([]);
		expect((await projectRecords(t.requests, unclaimed)).author).toEqual([]);
	});

	it("doesn't take a turned-down claim, or a change request from someone an admin named, as asking", async () => {
		const admin = await t.viewer(ADMIN);
		const mod = await t.viewer(MOD);
		const project = await t.projectAdmin.create({ actor: admin, profile: profile('Turned') });

		const claim = await t.requests.submit({
			actor: await t.viewer(CLAIMER),
			kind: 'claim',
			projectUri: project.uri,
			claim: { relationship: 'self', evidence: 'my site' }
		});

		await t.requests.decide({ actor: mod, uri: claim.uri, outcome: 'rejected', reason: 'No proof' });
		const named = await t.projectAdmin.setManagers({ actor: admin, project, managers: [CLAIMER, DJ] });

		const update = await t.requests.submit({
			actor: await t.viewer(DJ),
			kind: 'update',
			projectUri: named.uri,
			profile: { ...named.profile, tagline: 'New' }
		});

		await t.requests.decide({ actor: mod, uri: update.uri, outcome: 'approved' });
		const updated = (await t.projects.bySlug('turned'))!;

		expect(updated.profile.tagline).toBe('New');
		expect((await projectRecords(t.requests, updated)).author).toEqual([]);
	});

	it("drops someone whose claim was approved once they're no longer a manager", async () => {
		const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Gone') });

		const claim = await t.requests.submit({
			actor: await t.viewer(CLAIMER),
			kind: 'claim',
			projectUri: project.uri,
			claim: { relationship: 'self', evidence: 'my site' }
		});

		await t.requests.decide({ actor: await t.viewer(MOD), uri: claim.uri, outcome: 'approved' });

		const removed = await t.projectAdmin.setManagers({
			actor: await t.viewer(ADMIN),
			project: (await t.projects.bySlug('gone'))!,
			managers: []
		});

		expect((await projectRecords(t.requests, removed)).author).toEqual([]);
	});

	it('drops someone who withdrew their claim from their own repo after it was approved', async () => {
		const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Withdrawn') });

		const claim = await t.requests.submit({
			actor: await t.viewer(CLAIMER),
			kind: 'claim',
			projectUri: project.uri,
			claim: { relationship: 'self', evidence: 'my site' }
		});

		await t.requests.decide({ actor: await t.viewer(MOD), uri: claim.uri, outcome: 'approved' });
		const claimed = (await t.projects.bySlug('withdrawn'))!;
		expect((await projectRecords(t.requests, claimed)).author).toEqual([`at://${CLAIMER}`]);

		// As sync indexes it: the requester rewrote the record, so withdrawing reaches us after the approval.
		const withdrawnAt = '2026-02-01T00:00:00Z';
		await t.requests.index({
			uri: claim.uri,
			cid: 'bafywithdrawn',
			repoDid: CLAIMER,
			value: { ...claim.record, withdrawnAt, updatedAt: withdrawnAt }
		});

		expect((await projectRecords(t.requests, claimed)).author).toEqual([]);
	});

	it('takes another canonical record and the alternates a page shows, keeping only DID-based ones, once each', async () => {
		const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Kit') });
		const kit = `at://${new AtUri(project.uri).host}/computer.localnetwork.epk/3kit`;
		const set = `at://${DJ}/computer.localnetwork.dj.set/3set`;

		const records = await projectRecords(t.requests, project, {
			canonical: kit,
			alternate: [project.uri, set, set, undefined, 'at://neon.test/computer.localnetwork.dj.set/3x', 'not a uri']
		});

		expect(records).toEqual({ canonical: [kit], alternate: [project.uri, set], author: [] });
		expectDidUris(records);
	});
});

describe('withRecords', () => {
	const project: ProjectView = {
		uri: 'at://did:plc:root/computer.localnetwork.project/1',
		rkey: '1',
		cid: 'c',
		slug: 'neon-tide',
		profile: profile('Neon Tide'),
		managers: [],
		status: 'active',
		revision: 1,
		createdBy: ADMIN,
		createdAt: '2026-01-01T00:00:00Z',
		updatedAt: '2026-01-01T00:00:00Z',
		avatarUrl: null
	};

	const records: PageRecords = { canonical: [project.uri], alternate: [], author: [`at://${DJ}`] };

	it("adds the records to a page search engines may list, and none to one they're kept from", async () => {
		const listed: PageMeta = await withRecords(performerMeta('https://localnetwork.test', project), () => records);

		const hidden = await withRecords(
			performerMeta('https://localnetwork.test', { ...project, status: 'hidden' }),
			() => records
		);

		expect(listed.records).toEqual(records);
		expect(hidden.noindex).toBe(true);
		expect(hidden).not.toHaveProperty('records');
	});

	it("doesn't look up the records of a page kept out of search", async () => {
		// Any lookup fails, so the page resolving at all shows none was made.
		const unreachable = { joinedManagers: () => Promise.reject(new Error('looked up')) };

		const hidden = await withRecords(performerMeta('https://localnetwork.test', { ...project, status: 'hidden' }), () =>
			projectRecords(unreachable, { ...project, managers: [DJ] })
		);

		expect(hidden).not.toHaveProperty('records');
	});
});

describe('djProfiles', () => {
	const activity = (did: string, withProfile: boolean): DjActivity => ({
		did,
		handle: null,
		profile: withProfile ? { generatedAt: '2026-01-01T00:00:00Z' } : null,
		sets: []
	});

	it("names each manager's stats record when they're at hand, and none while they're streamed", () => {
		expect(djProfiles([activity(DJ, true), activity(CLAIMER, false)])).toEqual([
			`at://${DJ}/computer.localnetwork.dj.profile/self`
		]);
		expect(djProfiles(Promise.resolve([activity(DJ, true)]))).toEqual([]);
	});
});

describe('profileRecords', () => {
	it("makes a person's profile record canonical and them its author", () => {
		const records = profileRecords({ did: DJ });

		expect(records).toEqual({
			canonical: [`at://${DJ}/computer.localnetwork.profile/self`],
			alternate: [],
			author: [`at://${DJ}`]
		});
		expectDidUris(records);
	});

	it('names nothing for an account without a profile record', () => {
		expect(profileRecords(null)).toEqual({ canonical: [], alternate: [], author: [] });
	});
});
