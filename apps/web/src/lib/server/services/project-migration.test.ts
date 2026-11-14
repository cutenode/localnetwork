import { sql } from 'kysely';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { DEMO_DJS, DEMO_PROJECTS, DEMO_SHOWS } from '../../../../scripts/seed-data.ts';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProjectPlan } from '#lib/project-migration.ts';
import type { ProjectDetails, ProjectProfile, ProjectStatus, ProjectView } from '#lib/types.ts';
import { sharedSpace } from '../atproto/spaces';
import { pressKitServices, preset, values } from '../testing/press-kits';
import { validateRecord } from '../atproto/lexicons';
import { createTestServices, ROOT } from '../testing/core';

const ADMIN = 'did:plc:admin';

const SOLO = 'did:plc:solo';

const MEMBER = 'did:plc:member';

const MANAGER = 'did:plc:manager';

const AGENT = 'did:plc:agent';

const ADDED = 'did:plc:added';

const STRANGER = 'did:plc:stranger';

const PROMOTER = 'did:plc:promoter';

const profile = (name: string, kind: string, extra: Partial<ProjectProfile> = {}): ProjectProfile => ({
	name,
	kind,
	genres: ['techno'],
	location: { city: 'Berlin', country: 'DE' },
	...extra
});

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, SOLO, PROMOTER] });

	for (const [did, handle] of [
		[SOLO, 'solo.test'],
		[MANAGER, 'manager.test']
	])
		await t.accounts.recordLogin({
			did,
			handle,
			pdsUrl: null,
			grantedScope: 'atproto',
			spacesCapable: true,
			publicWrites: true
		});
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

const publish = async (
	project: ProjectProfile,
	input: { managers?: string[]; createdBy?: string; status?: ProjectStatus; details?: ProjectDetails } = {}
) =>
	(
		await t.projects.publish({
			profile: project,
			managers: input.managers ?? [],
			status: input.status ?? 'active',
			createdBy: input.createdBy ?? ADMIN,
			details: input.details
		})
	).project;

/** `did` claims `project` as `relationship`, and staff approve it unless `pending`. */
const claim = async (project: ProjectView, did: string, relationship: string, pending = false) => {
	const request = await t.requests.submit({
		actor: await t.viewer(did),
		kind: 'claim',
		projectUri: project.uri,
		claim: { relationship, evidence: 'It is me' }
	});

	if (!pending) await t.requests.decide({ actor: await t.viewer(ADMIN), uri: request.uri, outcome: 'approved' });

	return request.uri;
};

const planFor = async (project: ProjectView, overrides?: Record<string, string>): Promise<ProjectPlan> => {
	const plan = await t.projectMigration.plan({ djOverrides: overrides });

	return plan.projects.find((planned) => planned.uri === project.uri)!;
};

describe('a solo project', () => {
	it('becomes the DJ who claimed it as themselves, with the other managers as managers', async () => {
		const project = await publish(profile('Neon Tide', 'solo'), { managers: [MANAGER] });
		await claim(project, SOLO, 'self');

		const planned = await planFor(project);

		expect(planned).toMatchObject({
			target: 'dj',
			dj: { did: SOLO, handle: 'solo.test', rule: 'self' },
			performerUri: `at://${SOLO}/${COLLECTION.profile}/self`,
			record: {
				subject: { $type: `${COLLECTION.performer}#dj`, did: SOLO },
				formerly: [project.uri],
				createdAt: project.createdAt
			},
			space: sharedSpace(ROOT, 'performer', project.rkey),
			redirect: { from: '/projects/neon-tide', to: '/@solo.test' },
			blockers: []
		});
		expect(planned.people).toEqual([
			{ did: SOLO, handle: 'solo.test', role: 'member', relationship: 'self' },
			{ did: MANAGER, handle: 'manager.test', role: 'manager', relationship: null }
		]);
	});

	it('falls back to its creator when they manage it, then to staff naming the DJ', async () => {
		const created = await publish(profile('Pirate Signal', 'solo'), { managers: [MANAGER, SOLO], createdBy: SOLO });

		expect((await planFor(created)).dj).toEqual({ did: SOLO, handle: 'solo.test', rule: 'createdBy' });

		const named = await publish(profile('Velvet Static', 'solo'), { managers: [MANAGER] });

		expect((await planFor(named, { [named.uri]: MANAGER })).dj).toEqual({
			did: MANAGER,
			handle: 'manager.test',
			rule: 'override'
		});
	});

	it("is blocked when nobody claimed it as themselves and its creator doesn't manage it", async () => {
		const project = await publish(profile('Pirate Signal', 'solo'), { managers: [MANAGER] });

		expect(await planFor(project)).toMatchObject({
			target: 'dj',
			dj: { did: null, rule: 'unresolved' },
			record: null,
			performerUri: null,
			people: [{ did: MANAGER, role: 'manager' }],
			redirect: { from: '/projects/pirate-signal', to: null },
			blockers: [{ kind: 'unresolvedDj' }]
		});
	});

	it("lists every field it set as not carried, since the DJ's profile is their own", async () => {
		const project = await publish(profile('Neon Tide', 'solo', { tagline: 'Rolling techno' }));
		await claim(project, SOLO, 'self');

		expect((await planFor(project)).unmapped.map((field) => field.field)).toEqual([
			'name',
			'kind',
			'tagline',
			'genres',
			'location'
		]);
	});

	it('is blocked when its DJ already has a space of their own', async () => {
		const project = await publish(profile('Neon Tide', 'solo'), { managers: [SOLO], createdBy: SOLO });
		const space = sharedSpace(ROOT, 'performer', '3kdjspace');

		await t.db
			.insertInto('performer')
			.values({
				spaceUri: space,
				skey: '3kdjspace',
				kind: 'dj',
				slug: null,
				did: SOLO,
				record: JSON.stringify({ subject: { $type: `${COLLECTION.performer}#dj`, did: SOLO } }),
				formerly: '[]',
				cid: 'c',
				updatedAt: project.updatedAt
			})
			.execute();

		expect((await planFor(project)).blockers).toEqual([{ kind: 'djSpaceTaken', did: SOLO, space }]);
	});
});

describe('a duo', () => {
	it('makes claimed managers and agents managers, and everyone else members', async () => {
		const project = await publish(profile('Saffron Social', 'duo'), { managers: [ADDED] });
		await claim(project, MEMBER, 'member');
		await claim(project, MANAGER, 'manager');
		await claim(project, AGENT, 'agent');
		const pending = await claim(project, STRANGER, 'self', true);

		const planned = await planFor(project);

		expect(planned.people.map(({ did, role }) => [did, role])).toEqual([
			[ADDED, 'member'],
			[MEMBER, 'member'],
			[MANAGER, 'manager'],
			[AGENT, 'manager']
		]);
		expect(planned).toMatchObject({
			target: 'act',
			dj: null,
			performerUri: `at://${ROOT}/${COLLECTION.listing}/${project.rkey}`,
			record: {
				subject: { $type: `${COLLECTION.performer}#act`, slug: 'saffron-social', form: 'duo' },
				formerly: [project.uri]
			},
			redirect: { from: '/projects/saffron-social', to: '/act/saffron-social' },
			decline: [{ uri: pending, kind: 'claim', requester: STRANGER }],
			blockers: []
		});
	});

	it('maps the profile into basics and performer sections', async () => {
		const project = await publish(
			profile('Lowkey Lagos', 'collective', {
				tagline: 'Amapiano crew',
				bio: 'A crew',
				pronouns: 'they',
				links: [{ kind: 'website', url: 'https://lowkey.test' }],
				activeSince: 2019,
				fees: [{ tier: 'established' }],
				lookingFor: 'Day parties'
			}),
			{ managers: [MEMBER] }
		);

		const planned = await planFor(project);

		expect(planned.record?.subject).toEqual({
			$type: `${COLLECTION.performer}#act`,
			slug: 'lowkey-lagos',
			form: 'collective',
			basics: {
				name: 'Lowkey Lagos',
				bio: 'A crew',
				pronouns: 'they',
				location: { city: 'Berlin', country: 'DE' },
				links: [{ kind: 'website', url: 'https://lowkey.test' }]
			},
			performer: {
				tagline: 'Amapiano crew',
				genres: ['techno'],
				activeSince: 2019,
				fees: [{ tier: 'established' }],
				lookingFor: 'Day parties'
			}
		});
		expect(planned.unmapped).toEqual([]);
	});
});

describe('fields an act has no place for', () => {
	it('plans a live act as a group and says so', async () => {
		const planned = await planFor(await publish(profile('Glasshouse', 'live'), { managers: [MEMBER] }));

		expect(planned.record?.subject).toMatchObject({ form: 'group' });
		expect(planned.unmapped).toEqual([{ field: 'kind', reason: expect.stringContaining('"live"') }]);
	});

	it('folds a legacy fee into fees, and drops it when fees already lists them', async () => {
		const legacy = await publish(profile('Old Fee', 'duo', { fee: { tier: 'emerging' } }), { managers: [MEMBER] });

		const both = await publish(
			profile('Both Fees', 'duo', { fee: { tier: 'emerging' }, fees: [{ tier: 'headliner' }] }),
			{ managers: [MEMBER] }
		);

		const plan = await t.projectMigration.plan();
		const of = (project: ProjectView) => plan.projects.find((planned) => planned.uri === project.uri)!;

		expect(of(legacy).record?.subject).toMatchObject({ performer: { fees: [{ tier: 'emerging' }] } });
		expect(of(legacy).unmapped).toEqual([]);
		expect(of(both).record?.subject).toMatchObject({ performer: { fees: [{ tier: 'headliner' }] } });
		expect(of(both).unmapped).toEqual([{ field: 'fee', reason: expect.stringContaining('Dropped') }]);
	});
});

describe('what moves with a project', () => {
	it('lists only active projects', async () => {
		const plan = async (status: ProjectStatus) =>
			(await planFor(await publish(profile(`Status ${status}`, 'duo'), { managers: [MEMBER], status }))).listed;

		expect(await plan('active')).toBe(true);
		expect(await plan('hidden')).toBe(false);
		expect(await plan('archived')).toBe(false);
	});

	it('copies booking details only when there are some', async () => {
		const withDetails = await publish(profile('With', 'duo'), {
			managers: [MEMBER],
			details: { bookingEmail: 'book@with.test' }
		});

		const without = await publish(profile('Without', 'duo'), { managers: [MEMBER] });

		expect((await planFor(withDetails)).hasDetails).toBe(true);
		expect((await planFor(without)).hasDetails).toBe(false);
	});

	it('repoints booking threads, counts follows and upcoming shows, and blocks a project nobody manages', async () => {
		const project = await publish(profile('Booked', 'duo'), { managers: [SOLO] });
		const orphan = await publish(profile('Orphan', 'duo'));

		const space = await t.bookings.start({
			actor: await t.viewer(PROMOTER),
			projectUri: project.uri,
			show: { title: 'Night' },
			message: 'Free?'
		});

		await t.follows.follow(await t.viewer(PROMOTER), project);
		const { show } = DEMO_SHOWS[0];
		const date = `${new Date().getFullYear() + 1}-06-01`;
		await t.shows.save(await t.viewer(SOLO), project, { ...show, date, cancelled: false });

		const planned = await planFor(project);

		expect(planned.bookingThreads).toEqual([{ space, title: 'Night', status: 'open' }]);
		expect(planned.lazy).toEqual({ follows: 1, upcomingShows: 1 });
		expect((await planFor(orphan)).blockers).toEqual([{ kind: 'noManagers' }]);
	});

	it('lists pending create requests once for the whole site', async () => {
		const request = await t.requests.submit({
			actor: await t.viewer(STRANGER),
			kind: 'create',
			profile: profile('Brand New', 'duo')
		});

		expect((await t.projectMigration.plan()).pendingCreates).toEqual([
			{ uri: request.uri, kind: 'create', requester: STRANGER, createdAt: request.createdAt }
		]);
	});

	it('is blocked by an act that already has its slug', async () => {
		const project = await publish(profile('Saffron Social', 'duo'), { managers: [MEMBER] });

		const space = await t.sharedSpaces.createAct(await t.viewer(SOLO), {
			slug: 'saffron-social',
			form: 'duo',
			basics: { name: 'Saffron Social' }
		});

		expect((await planFor(project)).blockers).toEqual([{ kind: 'slugTaken', slug: 'saffron-social', space }]);
	});
});

describe('press kits and drafts', () => {
	it("repoints the root's press kits, copies drafts, and is blocked by one still in a manager's repo", async () => {
		await t.db.destroy();
		const f = await pressKitServices();
		t = f.t;
		const published = await f.publish(f.project.managers[0]);
		const legacy = await f.legacy(f.project.managers[1]);

		const draft = await t.epks.drafts.save(await t.viewer(f.project.managers[0]), f.project, {
			rkey: published.rkey,
			cid: published.cid,
			values: values({ title: 'Clubs, revised' })
		});

		const planned = await planFor(f.project, { [f.project.uri]: f.project.managers[0] });

		expect(planned.pressKits).toEqual([{ uri: published.uri, title: 'Clubs' }]);
		expect(planned.drafts).toEqual([
			{ uri: draft!.uri, author: f.project.managers[0], epkRkey: published.rkey, updatedAt: expect.any(String) }
		]);
		expect(planned.blockers).toEqual([{ kind: 'legacyPressKit', uri: legacy.uri, did: f.project.managers[1] }]);
	});
});

/** Every row of every table, and everything on the fake network, so a test can tell nothing changed. */
const everything = async () => {
	const { rows } = await sql<{ name: string }>`select name from sqlite_master where type = 'table'`.execute(t.db);
	const tables: Record<string, unknown[]> = {};

	for (const { name } of rows) tables[name] = (await sql`select * from ${sql.table(name)}`.execute(t.db)).rows;

	return {
		tables,
		records: [...t.net.records.entries()],
		spaceRecords: [...t.net.spaceRecords.entries()],
		spaces: [...t.net.spaces.entries()].map(([uri, space]) => [uri, space.policy, [...space.members]]),
		blobs: [...t.net.blobs.keys()]
	};
};

it('writes nothing to any repo, space or table', async () => {
	const project = await publish(profile('Saffron Social', 'duo'), {
		managers: [MANAGER],
		details: { bookingEmail: 'a@b.test' }
	});

	await claim(project, SOLO, 'self', true);
	await publish(profile('Neon Tide', 'solo'), { managers: [SOLO], createdBy: SOLO });
	await t.audit.idle();
	const before = await everything();

	const plan = await t.projectMigration.plan();
	await t.audit.idle();

	expect(plan.projects).toHaveLength(2);
	expect(await everything()).toEqual(before);
});

describe('the seed data', () => {
	it('plans the demo projects', async () => {
		const djs = new Map(DEMO_DJS.map((dj) => [dj.project, `did:plc:${dj.handle.split('.')[0]}`]));

		for (const [index, demo] of DEMO_PROJECTS.entries()) {
			const managers = [...(index < 2 ? ['did:plc:neon'] : []), ...(djs.has(index) ? [djs.get(index)!] : [])];
			await publish(demo, { managers });
		}

		// neon.test's press kit that the root doesn't hold yet, as the seed leaves it.
		const neonTide = (await t.projects.bySlug('neon-tide'))!;
		const now = new Date().toISOString();
		await t.net.client('did:plc:neon').putRecord({
			collection: COLLECTION.epk,
			rkey: '3klegacy',
			record: validateRecord(COLLECTION.epk, {
				project: neonTide.uri,
				title: 'Radio',
				access: preset('review'),
				createdAt: now,
				updatedAt: now
			})
		});

		const plan = await t.projectMigration.plan();

		const blockers = Object.fromEntries(
			plan.projects.map((planned) => [planned.slug, planned.blockers.map((found) => found.kind)])
		);

		expect(plan.totals).toEqual({ acts: 5, djs: 7, blocked: 11 });
		expect(blockers).toEqual({
			'neon-tide': ['unresolvedDj', 'legacyPressKit'],
			'saffron-social': [],
			'lowkey-lagos': ['noManagers'],
			'pirate-signal': ['unresolvedDj'],
			'velvet-static': ['noManagers', 'unresolvedDj'],
			'moss-and-marrow': ['noManagers'],
			halflight: ['noManagers', 'unresolvedDj'],
			'sundial-soundsystem': ['noManagers'],
			'harbour-lights': ['unresolvedDj'],
			'coyote-cumbia-club': ['noManagers'],
			glasshouse: ['unresolvedDj'],
			'footwork-fever': ['noManagers', 'unresolvedDj']
		});
		expect(plan.projects.find((planned) => planned.slug === 'moss-and-marrow')?.unmapped).toEqual([
			{ field: 'kind', reason: expect.stringContaining('"live"') }
		]);
	});
});
