import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { sampleReport } from '../../src/lib/mix/sample.ts';
import { COLLECTION, SPACE_TYPE } from '../../src/lib/nsid.ts';
import { ROOT_SPACE_POLICIES } from '../../src/lib/server/bootstrap.ts';
import { ForbiddenError, UserError } from '../../src/lib/server/core.ts';
import { plannerSpace } from '../../src/lib/server/atproto/spaces.ts';
import type { ProjectProfile } from '../../src/lib/types.ts';
import { TestNetwork } from './network.ts';

const profile = (name: string, overrides: Partial<ProjectProfile> = {}): ProjectProfile => ({
	name,
	kind: 'solo',
	genres: ['techno', 'electro'],
	location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 },
	fee: { tier: 'emerging' },
	setTypes: ['peak'],
	...overrides
});

describe('Local Network on a real atproto spaces network', () => {
	let net: TestNetwork;

	beforeAll(async () => {
		net = await TestNetwork.create(['ada', 'milo', 'neon', 'pat', 'lee', 'remy', 'kai'], {
			admins: ['ada'],
			oauth: ['remy']
		});
	});
	afterAll(async () => {
		await net?.close();
	});

	/** Everything the app requests except blob uploads. */
	const withoutBlobs = () =>
		net.app.oauth.scopes.full
			.split(' ')
			.filter((scope) => !scope.startsWith('blob'))
			.join(' ');

	it('bootstraps the root spaces with the intended policies', async () => {
		const root = await net.app.core.root();

		const policies = new Map(Object.entries(ROOT_SPACE_POLICIES));

		for (const [name, uri] of Object.entries(net.app.spaces)) {
			const space = await root.getSpace(uri);
			expect(space?.readPolicy.$type).toBe(policies.get(name)?.readPolicy.$type);
			expect(space?.writePolicy.$type).toBe(policies.get(name)?.writePolicy.$type);
		}

		const bootstrap = await net.app.audit.list({ action: 'system' });
		expect(bootstrap.map((entry) => entry.action)).toContain('system.bootstrap');
	});

	it('lets an admin grant a moderator role, stored in the staff space and mirrored to member lists', async () => {
		const ada = await net.viewer('ada');
		await net.app.staff.grant(ada, net.user('milo').did, 'moderator', 'Weekend shifts');
		expect(await net.app.staff.roles(net.user('milo').did)).toEqual(['moderator']);
		const root = await net.app.core.root();

		const role = await root.getSpaceRecord({
			space: net.app.spaces.staff,
			repo: net.rootDid,
			collection: COLLECTION.staffRole,
			rkey: net.user('milo').did
		});

		expect(role?.value).toMatchObject({ subject: net.user('milo').did, role: 'moderator', grantedBy: ada.did });
		const members = await root.listMembers(net.app.spaces.moderation);
		expect(members).toContainEqual({ did: net.user('milo').did, read: true, write: true });
	});

	let createRequestUri = '';
	let projectUri = '';

	it("files a new-project request in the requester's own repo inside the moderation space", async () => {
		const neon = await net.viewer('neon');

		const request = await net.app.requests.submit({
			actor: neon,
			kind: 'create',
			profile: profile('Static Bloom', { tagline: 'Electro for night owls' }),
			details: { bookingEmail: 'bookings@staticbloom.test', techRider: '2x CDJ' },
			message: 'First project!'
		});

		createRequestUri = request.uri;
		expect(request.repoDid).toBe(neon.did);
		expect(request.status).toBe('pending');

		const own = await net.user('neon').client.getSpaceRecord({
			space: net.app.spaces.moderation,
			repo: neon.did,
			collection: COLLECTION.request,
			rkey: request.rkey
		});

		expect(own?.value).toMatchObject({ kind: 'create', profile: { name: 'Static Bloom' } });
		// Other members cannot read it from their PDS sessions.
		await expect(
			net.user('pat').client.listSpaceRecords({ space: net.app.spaces.moderation, repo: neon.did })
		).rejects.toBeTruthy();
	});

	it('refuses reviews by non-staff (audited as denied) and by the requester', async () => {
		const pat = await net.viewer('pat');
		await expect(
			net.app.requests.decide({ actor: pat, uri: createRequestUri, outcome: 'approved' })
		).rejects.toBeInstanceOf(ForbiddenError);
		const denied = await net.app.audit.list({ actor: pat.did, outcome: 'denied' });
		expect(denied).toHaveLength(1);
		await expect(
			net.app.requests.decide({ actor: await net.viewer('neon'), uri: createRequestUri, outcome: 'approved' })
		).rejects.toBeInstanceOf(ForbiddenError);
		await expect(
			net.app.requests.decide({ actor: await net.viewer('milo'), uri: createRequestUri, outcome: 'rejected' })
		).rejects.toBeInstanceOf(UserError);
	});

	it('publishes the project from the root account when a moderator approves', async () => {
		const milo = await net.viewer('milo');

		const decided = await net.app.requests.decide({
			actor: milo,
			uri: createRequestUri,
			outcome: 'approved',
			reason: 'Welcome!'
		});

		expect(decided.status).toBe('approved');
		const project = await net.app.projects.bySlug('static-bloom');
		expect(project).not.toBeNull();
		projectUri = project!.uri;
		expect(project).toMatchObject({ managers: [net.user('neon').did], revision: 1, status: 'active' });

		const root = await net.app.core.root();
		const published = await root.getRecord({ collection: COLLECTION.project, rkey: project!.rkey });
		expect(published?.value).toMatchObject({
			slug: 'static-bloom',
			lastRequest: createRequestUri,
			createdBy: net.user('neon').did
		});

		const details = await root.getSpaceRecord({
			space: net.app.spaces.industry,
			repo: net.rootDid,
			collection: COLLECTION.projectDetails,
			rkey: project!.rkey
		});

		expect(details?.value).toMatchObject({
			project: project!.uri,
			details: { bookingEmail: 'bookings@staticbloom.test' }
		});
		expect(await net.app.projects.details(project!.uri)).toMatchObject({ techRider: '2x CDJ' });
	});

	it('writes every audit entry twice (actor repo + chained root copy) and verifies them via sync', async () => {
		await net.settle();
		const entries = await net.app.audit.list({ limit: 500 });

		const neonSubmit = entries.find(
			(entry) => entry.action === 'request.submit' && entry.actorDid === net.user('neon').did
		)!;

		expect(neonSubmit.actorUri).toContain(`/${net.user('neon').did}/${COLLECTION.auditEntry}/${neonSubmit.id}`);
		expect(neonSubmit.rootUri).toContain(`/${net.rootDid}/${COLLECTION.auditEntry}/${neonSubmit.id}`);
		expect(neonSubmit.actorStatus).toBe('verified');
		expect(neonSubmit.rootStatus).toBe('verified');
		const approval = entries.find((entry) => entry.action === 'request.approve')!;
		expect(approval.actorDid).toBe(net.user('milo').did);
		expect(approval.actorStatus).toBe('verified');
		expect(await net.app.audit.verifyChain()).toMatchObject({ ok: true });
		expect(await net.app.audit.stats()).toMatchObject({ pending: 0, failed: 0 });
	});

	it('handles claims, change requests, and requested changes', async () => {
		const pat = await net.viewer('pat');

		const claim = await net.app.requests.submit({
			actor: pat,
			kind: 'claim',
			projectUri,
			claim: { relationship: 'agent', evidence: 'I book them; check the bio link', links: ['https://example.test/bio'] }
		});

		await expect(
			net.app.requests.submit({
				actor: pat,
				kind: 'claim',
				projectUri,
				claim: { relationship: 'agent', evidence: 'again' }
			})
		).rejects.toBeInstanceOf(UserError);
		const ada = await net.viewer('ada');
		await net.app.requests.decide({
			actor: ada,
			uri: claim.uri,
			outcome: 'changesRequested',
			reason: 'Add a link from their own account'
		});
		expect((await net.app.requests.get(claim.uri))?.status).toBe('changes_requested');
		await net.app.requests.revise(pat, claim.uri, {
			claim: { relationship: 'agent', evidence: 'Link added on their SoundCloud' }
		});
		expect((await net.app.requests.get(claim.uri))?.status).toBe('pending');
		await net.app.requests.decide({ actor: ada, uri: claim.uri, outcome: 'approved' });
		expect((await net.app.projects.byUri(projectUri))?.managers).toEqual([net.user('neon').did, pat.did]);

		const neon = await net.viewer('neon');

		const update = await net.app.requests.submit({
			actor: neon,
			kind: 'update',
			projectUri,
			profile: profile('Static Bloom', { tagline: 'Electro and breaks for night owls', genres: ['electro', 'breaks'] })
		});

		await expect(
			net.app.requests.submit({
				actor: await net.viewer('lee'),
				kind: 'update',
				projectUri,
				profile: profile('Hijack')
			})
		).rejects.toBeInstanceOf(ForbiddenError);
		await net.app.requests.decide({ actor: ada, uri: update.uri, outcome: 'approved' });
		const updated = await net.app.projects.byUri(projectUri);
		expect(updated).toMatchObject({
			revision: 3,
			profile: { tagline: 'Electro and breaks for night owls', genres: ['electro', 'breaks'] }
		});
		const projectTrail = await net.app.audit.list({ subject: projectUri });
		expect(projectTrail.map((entry) => entry.action)).toEqual(
			expect.arrayContaining(['project.create', 'project.managers', 'project.update'])
		);
		expect(projectTrail.find((entry) => entry.action === 'project.update')?.changes).toContain('genres');
	});

	it("searches and matches through HappyView's index", async () => {
		const near = { lat: 52.52, lng: 13.405 };
		const found = await net.app.projects.search({ q: 'night owls', near, genres: ['electro'] });
		expect(found.items.map((project) => project.uri)).toEqual([projectUri]);
		expect(found.items[0].distanceKm).toBeLessThan(1);

		const [best] = await net.app.projects.match({
			location: near,
			radiusKm: 25,
			genres: ['electro'],
			budgetTier: 'emerging'
		});

		expect(best.candidate.uri).toBe(projectUri);
		expect(best.reasons).toEqual(['Plays electro', 'Local', 'Fits budget']);
	});

	it('copies a requester-uploaded avatar into the root repo when approved', async () => {
		const png = new Uint8Array(
			Buffer.from(
				'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
				'base64'
			)
		);

		const neon = await net.viewer('neon');

		const request = await net.app.requests.submit({
			actor: neon,
			kind: 'create',
			profile: profile('Pixel Party'),
			avatar: { bytes: png, mimeType: 'image/png' }
		});

		expect(request.record.profile?.avatar?.ref.$link).toMatch(/^bafkrei/);
		await net.app.requests.decide({ actor: await net.viewer('ada'), uri: request.uri, outcome: 'approved' });
		const project = await net.app.projects.bySlug('pixel-party');
		expect(project?.avatarUrl).toBe(`/media/${net.rootDid}/${request.record.profile!.avatar!.ref.$link}`);

		const blob = await fetch(
			`${net.pds.url}/xrpc/com.atproto.sync.getBlob?did=${net.rootDid}&cid=${request.record.profile!.avatar!.ref.$link}`
		);

		expect(blob.status).toBe(200);
		expect(new Uint8Array(await blob.arrayBuffer())).toEqual(png);
	});

	it('lets a DJ signed in with OAuth propose a mix tracklist that root publishes on approval', async () => {
		const remy = await net.viewer('remy');

		const request = await net.app.requests.submit({
			actor: remy,
			kind: 'create',
			profile: profile('Tape Deck', { mixes: [{ title: 'Warehouse', url: 'https://mixes.test/warehouse' }] }),
			tracklists: [{ bytes: new TextEncoder().encode(JSON.stringify(sampleReport)), mimeType: 'application/json' }]
		});

		expect(request.repoDid).toBe(remy.did);
		expect(request.record.profile?.mixes?.[0]?.tracklist?.mimeType).toBe('application/json');
		await net.app.requests.decide({ actor: await net.viewer('ada'), uri: request.uri, outcome: 'approved' });
		const tracklist = (await net.app.projects.bySlug('tape-deck'))?.profile.mixes?.[0]?.tracklist;
		expect(tracklist).toBeDefined();
		expect((await net.app.tracklists.published(tracklist!)).plays).toHaveLength(sampleReport.plays.length);
	});

	it('asks someone to sign in again when the app requests scopes their OAuth grant predates', async () => {
		const kai = net.user('kai');

		const tracklists = [
			{ bytes: new TextEncoder().encode(JSON.stringify(sampleReport)), mimeType: 'application/json' }
		];

		const mixes = [{ title: 'Basement', url: 'https://mixes.test/basement' }];
		// Grants from before tracklists can't upload JSON. Approving everything but blob uploads gets a
		// grant like that from the PDS, recorded the way sign-ins were then: spaces-capable.
		const granted = await net.signInWithOAuth('kai', withoutBlobs());
		await net.app.accounts.recordLogin({
			did: kai.did,
			handle: kai.handle,
			pdsUrl: net.pds.url,
			grantedScope: granted,
			spacesCapable: true
		});
		const stale = { did: kai.did, handle: kai.handle, roles: [], isAdmin: false, isStaff: false, spacesCapable: true };
		await expect(
			net.app.requests.submit({ actor: stale, kind: 'create', profile: profile('Kai Tapes', { mixes }), tracklists })
		).rejects.toThrow('Missing required scope "blob:application/json"');

		expect(await net.app.viewer(kai.did)).toBeNull();

		await net.signInWithOAuth('kai');
		const viewer = await net.viewer('kai');
		expect(viewer.spacesCapable).toBe(true);

		const request = await net.app.requests.submit({
			actor: viewer,
			kind: 'create',
			profile: profile('Kai Tapes', { mixes }),
			tracklists
		});

		expect(request.repoDid).toBe(kai.did);
		expect(request.record.profile?.mixes?.[0]?.tracklist?.mimeType).toBe('application/json');
	});

	it('treats an OAuth grant missing scopes the user declined as one without spaces support', async () => {
		// If it counted as spaces-capable, it would always look stale and every sign-in would land back
		// on the login page.
		await net.signInWithOAuth('kai', withoutBlobs());
		expect((await net.viewer('kai')).spacesCapable).toBe(false);
	});

	it('lets a requester withdraw a pending request', async () => {
		const lee = await net.viewer('lee');
		const request = await net.app.requests.submit({ actor: lee, kind: 'create', profile: profile('Lee Lights') });
		await net.app.requests.withdraw(lee, request.uri);
		expect((await net.app.requests.get(request.uri))?.status).toBe('withdrawn');
		await net.settle();
		expect((await net.app.requests.get(request.uri))?.status).toBe('withdrawn');
	});

	let bookingSpace = '';

	it('opens a private booking space shared only by the promoter and the managers', async () => {
		const lee = await net.viewer('lee');
		bookingSpace = await net.app.bookings.start({
			actor: lee,
			projectUri,
			show: { title: 'Rooftop Rave', date: '2026-12-01T22:00:00.000Z', capacity: 300, budgetTier: 'emerging' },
			message: 'Are you free on the 1st?'
		});
		expect(bookingSpace).toContain(`/space/${SPACE_TYPE.booking}/`);
		const root = await net.app.core.root();
		const members = (await root.listMembers(bookingSpace)).map((member) => member.did).sort();
		expect(members).toEqual([net.user('neon').did, net.user('pat').did, lee.did].sort());

		await net.app.bookings.message({
			actor: await net.viewer('neon'),
			space: bookingSpace,
			text: 'Yes! Send details.'
		});
		await net.app.bookings.setStatus({ actor: await net.viewer('pat'), space: bookingSpace, status: 'negotiating' });
		await expect(
			net.app.bookings.message({ actor: await net.viewer('ada'), space: bookingSpace, text: 'hi' })
		).rejects.toBeInstanceOf(ForbiddenError);

		await net.settle();
		const { thread, events } = await net.app.bookings.thread(lee, bookingSpace);
		expect(thread.status).toBe('negotiating');
		expect(events.map((event) => event.kind)).toEqual(['inquiry', 'message', 'status']);
		expect(events[1]).toMatchObject({ text: 'Yes! Send details.', author: { did: net.user('neon').did } });
		const status = await net.app.syncer.status();
		expect(status.find((space) => space.space === bookingSpace)?.repos.length).toBeGreaterThanOrEqual(3);
	});

	it("keeps a promoter's planner in a personal space on their own PDS", async () => {
		const lee = await net.viewer('lee');
		await net.app.planner.save(lee, projectUri, 'Great for rooftops');

		const rkey = await net.app.planner.saveShow(lee, {
			details: { title: 'Rooftop Rave', capacity: 300 },
			lineup: [{ project: projectUri, slot: '22:00', status: 'contacted' }]
		});

		expect(await net.app.planner.saves(lee)).toEqual([
			expect.objectContaining({ project: projectUri, note: 'Great for rooftops' })
		]);
		expect((await net.app.planner.show(lee, rkey))?.lineup[0]).toMatchObject({ status: 'contacted' });
		const space = await net.user('lee').client.getSpace(plannerSpace(lee.did));
		expect(space?.readPolicy.$type).toBe('com.atproto.simplespace.defs#memberListPolicy');
		await net.app.planner.unsave(lee, projectUri);
		expect(await net.app.planner.saves(lee)).toEqual([]);
	});

	it('files on behalf of accounts whose PDS lacks spaces support, and says so in the audit log', async () => {
		const legacy = { ...(await net.viewer('lee')), spacesCapable: false };
		await net.app.accounts.recordLogin({
			did: legacy.did,
			handle: 'lee.test',
			pdsUrl: null,
			grantedScope: 'atproto',
			spacesCapable: false
		});
		const request = await net.app.requests.submit({ actor: legacy, kind: 'create', profile: profile('Proxy Party') });
		expect(request.repoDid).toBe(net.rootDid);
		expect(request.requester.did).toBe(legacy.did);
		expect(request.record.onBehalfOf).toBe(legacy.did);
		await net.app.audit.idle();
		const [entry] = await net.app.audit.list({ actor: legacy.did, action: 'request.submit' });
		expect(entry.actorStatus).toBe('unsupported');
		expect(entry.rootStatus).not.toBe('pending');
		await net.app.accounts.recordLogin({
			did: legacy.did,
			handle: 'lee.test',
			pdsUrl: null,
			grantedScope: net.app.oauth.scopes.full,
			spacesCapable: true
		});
	});

	it('flags an audit copy that its author edits after the fact', async () => {
		await net.settle();
		const [entry] = await net.app.audit.list({ actor: net.user('pat').did, action: 'request.update' });

		const original = await net.user('pat').client.getSpaceRecord({
			space: net.app.spaces.audit,
			repo: net.user('pat').did,
			collection: COLLECTION.auditEntry,
			rkey: entry.id
		});

		await net.user('pat').client.putSpaceRecord({
			space: net.app.spaces.audit,
			collection: COLLECTION.auditEntry,
			rkey: entry.id,
			record: { ...original!.value, summary: 'Nothing to see here' }
		});
		await net.settle();
		const after = await net.app.audit.get(entry.id);
		expect(after?.actorStatus).toBe('failed');
		expect(after?.actorError).toMatch(/does not match/);
		expect(await net.app.audit.verifyChain()).toMatchObject({ ok: true });
	});

	// Published projects never lived in this index: HappyView keeps serving them.
	it('rebuilds the local index from atproto after losing it', async () => {
		const { db } = net.app;

		const before = {
			requests: await net.app.requests.counts(),
			audit: (await net.app.audit.stats()).total,
			roles: await net.app.staff.roles(net.user('milo').did)
		};

		for (const table of [
			'auditEntry',
			'auditCopy',
			'request',
			'staffRole',
			'spaceRecord',
			'syncedRepo',
			'bookingEvent',
			'bookingParticipant',
			'bookingThread',
			'projectDetails'
		] as const) {
			await db.deleteFrom(table).execute();
		}

		await db.updateTable('spaceWatch').set({ spaceRev: null }).execute();
		await net.app.syncer.syncAll();
		// Decisions reference requests, so a second pass applies any that synced first.
		await db.deleteFrom('syncedRepo').execute();
		await db.deleteFrom('spaceRecord').execute();
		await net.app.syncer.syncAll();

		expect(await net.app.requests.counts()).toEqual(before.requests);
		expect(await net.app.staff.roles(net.user('milo').did)).toEqual(before.roles);
		expect((await net.app.audit.stats()).total).toBe(before.audit);
		expect(await net.app.audit.verifyChain()).toMatchObject({ ok: true });
		expect(await net.app.projects.bySlug('static-bloom')).toMatchObject({ revision: 3 });
		expect(await net.app.projects.details(projectUri)).toMatchObject({ bookingEmail: 'bookings@staticbloom.test' });
		const { events } = await net.app.bookings.thread(await net.viewer('lee'), bookingSpace);
		expect(events).toHaveLength(3);
	});
});
