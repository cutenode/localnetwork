import type { JsonObject } from '@atproto/lex-json';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import {
	backstageSpace,
	bookingSpace,
	epkRequestSpace,
	epkSpace,
	parseSpaceRecordUri,
	sharedSpace
} from '../atproto/spaces';
import { grantKey } from '../services/epk';
import { createTestServices, ROOT } from '../testing/core';
import { CODJ, DJ, FRIEND, NO_SPACES, pressKitServices, STRANGER } from '../testing/press-kits';
import { createProjections } from './projections';
import { flagsOff } from '#lib/testing/flags.ts';

const ALICE = 'did:plc:alice';

const BOB = 'did:plc:bob';

describe('createProjections', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let project: ReturnType<typeof createProjections>;

	beforeEach(async () => {
		t = await createTestServices({ capable: [ALICE] });
		project = createProjections({
			core: t.core,
			audit: t.audit,
			projects: t.projects,
			requests: t.requests,
			staff: t.staff,
			flags: t.flags,
			bookings: t.bookings,
			epks: t.epks,
			sharedSpaces: t.sharedSpaces,
			listings: t.listings
		});
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const upsert = (space: string, repoDid: string, collection: string, rkey: string, value: JsonObject) =>
		project({
			kind: 'upsert',
			space,
			repoDid,
			collection,
			rkey,
			uri: `${space}/${repoDid}/${collection}/${rkey}`,
			cid: 'bafyreiabc',
			value: { $type: collection, ...value }
		});

	it('takes flags and groups from the root in the staff space, and nobody else', async () => {
		const at = new Date().toISOString();

		const flag = {
			key: 'parityCanary',
			everyone: true,
			percentage: 0,
			groups: [],
			dids: [],
			updatedBy: ROOT,
			updatedAt: at
		};

		const group = { name: 'Beta', members: [ALICE], createdAt: at, updatedBy: ROOT, updatedAt: at };

		await upsert(t.spaces.staff, ALICE, COLLECTION.staffFlag, 'parityCanary', flag);
		await upsert(t.spaces.moderation, ROOT, COLLECTION.staffFlag, 'parityCanary', flag);
		await upsert(t.spaces.staff, ALICE, COLLECTION.staffGroup, '3lzy2ji4nms2z', group);
		await upsert(t.spaces.moderation, ROOT, COLLECTION.staffGroup, '3lzy2ji4nms2z', group);

		expect(await t.flags.evaluate(null)).toEqual({ ...flagsOff, parityCanary: false });
		expect(await t.flags.groups()).toEqual([]);

		await upsert(t.spaces.staff, ROOT, COLLECTION.staffFlag, 'parityCanary', flag);
		await upsert(t.spaces.staff, ROOT, COLLECTION.staffGroup, '3lzy2ji4nms2z', group);

		expect(await t.flags.evaluate(null)).toEqual({ ...flagsOff, parityCanary: true });
		expect((await t.flags.groups()).map(({ name }) => name)).toEqual(['Beta']);

		for (const [repoDid, space] of [
			[ALICE, t.spaces.staff],
			[ROOT, t.spaces.moderation]
		]) {
			await project({
				kind: 'delete',
				space,
				repoDid,
				collection: COLLECTION.staffFlag,
				rkey: 'parityCanary',
				uri: `${t.spaces.staff}/${ROOT}/${COLLECTION.staffFlag}/parityCanary`
			});
		}

		expect(await t.flags.evaluate(null)).toEqual({ ...flagsOff, parityCanary: true });
	});

	it('indexes valid records and enforces authority rules', async () => {
		const now = new Date().toISOString();
		await upsert(t.spaces.moderation, ALICE, COLLECTION.request, 'r1', {
			kind: 'create',
			profile: { name: 'A', kind: 'solo', genres: ['x'], location: { city: 'c', country: 'DE' } },
			createdAt: now
		});
		expect((await t.requests.byRkey('r1'))?.status).toBe('pending');

		// Decisions and roles from anyone but the root are ignored.
		const decision = {
			request: `${t.spaces.moderation}/${ALICE}/${COLLECTION.request}/r1`,
			requester: ALICE,
			outcome: 'approved',
			decidedBy: ALICE,
			createdAt: now
		};

		await upsert(t.spaces.moderation, ALICE, COLLECTION.decision, 'd1', decision);
		expect((await t.requests.byRkey('r1'))?.status).toBe('pending');
		await upsert(t.spaces.moderation, ROOT, COLLECTION.decision, 'd2', {
			...decision,
			outcome: 'rejected',
			reason: 'no'
		});
		expect((await t.requests.byRkey('r1'))?.status).toBe('rejected');

		await upsert(t.spaces.staff, ALICE, COLLECTION.staffRole, ALICE, {
			subject: ALICE,
			role: 'admin',
			grantedBy: ALICE,
			createdAt: now
		});
		expect(await t.staff.roles(ALICE)).toEqual([]);
		await upsert(t.spaces.staff, ROOT, COLLECTION.staffRole, ALICE, {
			subject: ALICE,
			role: 'moderator',
			grantedBy: ROOT,
			createdAt: now
		});
		expect(await t.staff.roles(ALICE)).toEqual(['moderator']);
		await project({
			kind: 'delete',
			space: t.spaces.staff,
			repoDid: ROOT,
			collection: COLLECTION.staffRole,
			rkey: ALICE,
			uri: `${t.spaces.staff}/${ROOT}/${COLLECTION.staffRole}/${ALICE}`
		});
		expect(await t.staff.roles(ALICE)).toEqual([]);

		await upsert(t.spaces.industry, ROOT, COLLECTION.projectDetails, 'p1', {
			project: 'at://did:plc:root/computer.localnetwork.project/p1',
			details: { bookingEmail: 'x@y.test' },
			updatedAt: now
		});
		expect(await t.projects.details('at://did:plc:root/computer.localnetwork.project/p1')).toEqual({
			bookingEmail: 'x@y.test'
		});
	});

	it('ignores invalid records and booking records from non-participants', async () => {
		await upsert(t.spaces.moderation, ALICE, COLLECTION.request, 'bad', { kind: 'create' });
		expect(await t.requests.byRkey('bad')).toBeNull();
		await upsert(bookingSpace(ROOT, 'b1'), ALICE, COLLECTION.bookingMessage, 'm1', {
			text: 'spam',
			createdAt: new Date().toISOString()
		});
		expect(await t.db.selectFrom('bookingEvent').selectAll().execute()).toEqual([]);
	});

	it('ignores request kinds and staff roles the lexicons allow but the app does not know', async () => {
		const now = new Date().toISOString();
		await upsert(t.spaces.moderation, ALICE, COLLECTION.request, 'odd', { kind: 'merge', createdAt: now });
		expect(await t.requests.byRkey('odd')).toBeNull();
		await upsert(t.spaces.staff, ROOT, COLLECTION.staffRole, ALICE, {
			subject: ALICE,
			role: 'owner',
			grantedBy: ROOT,
			createdAt: now
		});
		expect(await t.staff.roles(ALICE)).toEqual([]);
	});

	it('drops a staff role when its record changes to a role the app does not know', async () => {
		const role = (name: string) => ({
			subject: ALICE,
			role: name,
			grantedBy: ROOT,
			createdAt: new Date().toISOString()
		});

		await upsert(t.spaces.staff, ROOT, COLLECTION.staffRole, ALICE, role('admin'));
		expect(await t.staff.roles(ALICE)).toEqual(['admin']);
		await upsert(t.spaces.staff, ROOT, COLLECTION.staffRole, ALICE, role('owner'));
		expect(await t.staff.roles(ALICE)).toEqual([]);
	});

	it('marks requests withdrawn when their record disappears and audit copies failed when deleted', async () => {
		const now = new Date().toISOString();
		await upsert(t.spaces.moderation, ALICE, COLLECTION.request, 'r2', {
			kind: 'create',
			profile: { name: 'A', kind: 'solo', genres: ['x'], location: { city: 'c', country: 'DE' } },
			createdAt: now
		});
		await project({
			kind: 'delete',
			space: t.spaces.moderation,
			repoDid: ALICE,
			collection: COLLECTION.request,
			rkey: 'r2',
			uri: `${t.spaces.moderation}/${ALICE}/${COLLECTION.request}/r2`
		});
		expect((await t.requests.byRkey('r2'))?.status).toBe('withdrawn');

		const entry = await t.audit.record({ actor: ALICE, action: 'auth.login', summary: 'in' }, { awaitDelivery: true });
		const stored = await t.audit.get(entry.id);
		await project({
			kind: 'delete',
			space: t.spaces.audit,
			repoDid: ALICE,
			collection: COLLECTION.auditEntry,
			rkey: entry.id,
			uri: stored!.actorUri!
		});
		expect((await t.audit.get(entry.id))?.actorStatus).toBe('failed');
	});

	it("counts a press kit grant only from its space's authority, and drops it when deleted", async () => {
		const space = epkSpace(ALICE, '3kit');
		const grant = { subject: BOB, via: 'approved', createdAt: new Date().toISOString() };

		await upsert(space, BOB, COLLECTION.epkGrant, BOB, grant);
		expect(await t.db.selectFrom('epkGrant').selectAll().execute()).toEqual([]);

		await upsert(space, ALICE, COLLECTION.epkGrant, BOB, grant);
		expect(await t.db.selectFrom('epkGrant').select(['subjectDid', 'via']).execute()).toEqual([
			{ subjectDid: BOB, via: 'approved' }
		]);

		await project({
			kind: 'delete',
			space,
			repoDid: ALICE,
			collection: COLLECTION.epkGrant,
			rkey: BOB,
			uri: `${space}/${ALICE}/${COLLECTION.epkGrant}/${BOB}`
		});
		expect(await t.db.selectFrom('epkGrant').selectAll().execute()).toEqual([]);
	});

	it("counts a press kit grant in its project's request space for the press kit it names", async () => {
		const f = await pressKitServices();
		const epk = await f.publish(DJ);
		const space = epkRequestSpace(ROOT, f.project.rkey);
		const rkey = grantKey(epk.rkey, BOB);
		const uri = `${space}/${ROOT}/${COLLECTION.epkGrant}/${rkey}`;
		const sync = createProjections(f.t);
		const grant = { subject: BOB, epk: epk.uri, via: 'invited', createdAt: new Date().toISOString() };

		try {
			await sync({
				kind: 'upsert',
				space,
				repoDid: ROOT,
				collection: COLLECTION.epkGrant,
				rkey,
				uri,
				cid: 'bafyreiabc',
				value: { $type: COLLECTION.epkGrant, ...grant }
			});
			expect(await f.t.db.selectFrom('epkGrant').select(['spaceUri', 'subjectDid']).execute()).toEqual([
				{ spaceUri: epk.space, subjectDid: BOB }
			]);

			await sync({ kind: 'delete', space, repoDid: ROOT, collection: COLLECTION.epkGrant, rkey, uri });
			expect(await f.t.db.selectFrom('epkGrant').selectAll().execute()).toEqual([]);
		} finally {
			await f.t.audit.idle();
			await f.t.db.destroy();
		}
	});

	describe('press kit decisions', () => {
		let f: Awaited<ReturnType<typeof pressKitServices>>;
		let kit: ReturnType<typeof f.kit>;
		let space: string;
		let sync: ReturnType<typeof createProjections>;

		beforeEach(async () => {
			f = await pressKitServices();
			kit = f.kit(await f.publish(DJ));
			space = epkRequestSpace(ROOT, f.project.rkey);
			sync = createProjections(f.t);
		});
		afterEach(async () => {
			await f.t.audit.idle();
			await f.t.db.destroy();
		});

		/** `did` asks to see the press kit; returns their request's URI. */
		const ask = async (did: string) => {
			await f.t.epks.request(await f.t.viewer(did), kit, {});
			const rows = await f.t.db.selectFrom('epkRequest').select('uri').where('requesterDid', '=', did).execute();

			return rows[0].uri;
		};

		const answer = async (did: string, uri: string, outcome: 'approve' | 'decline') =>
			f.t.epks.decide(await f.t.viewer(did), kit, uri, outcome);

		const states = async () =>
			Object.fromEntries(
				await Promise.all(
					[STRANGER, NO_SPACES, FRIEND].map(async (did) => {
						const state = await f.t.epks.accessState(kit, await f.t.viewer(did));

						return [did, state.kind] as const;
					})
				)
			);

		/**
		 * What the access page shows of each answered request. A request whose record is gone comes back from its decision
		 * alone, which doesn't say when it was asked or what the rules said then.
		 */
		const answers = async () =>
			(await f.t.epks.requests(kit.epk)).map(({ uri, requester, status, decidedAt }) => ({
				uri,
				requester: requester.did,
				status,
				decidedAt
			}));

		/** Empties the press kit tables and syncs every record in the request space back in, decisions first or last. */
		const rebuild = async (decisions: 'first' | 'last') => {
			await f.t.db.deleteFrom('epkRequest').execute();
			await f.t.db.deleteFrom('epkGrant').execute();
			const isDecision = (uri: string) => parseSpaceRecordUri(uri)?.collection === COLLECTION.epkDecision;

			const records = f.t.net
				.spaceRecordsIn(space)
				.toSorted((a, b) => Number(isDecision(b.uri)) - Number(isDecision(a.uri)));

			for (const { uri, cid, value } of decisions === 'first' ? records : records.toReversed()) {
				const { author, collection, rkey } = parseSpaceRecordUri(uri)!;
				await sync({ kind: 'upsert', space, repoDid: author, collection, rkey, uri, cid, value });
			}
		};

		const decision = (request: string, outcome: string) => ({
			request,
			requester: STRANGER,
			epk: kit.epk.uri,
			outcome,
			decidedBy: DJ,
			createdAt: new Date().toISOString()
		});

		it.each(['first', 'last'] as const)(
			'keeps every answer when the request space syncs again, its decisions %s',
			async (decisions) => {
				await answer(CODJ, await ask(STRANGER), 'decline');
				// The root holds this one, and its asker deletes it after the decline.
				const held = await ask(NO_SPACES);
				await answer(DJ, held, 'decline');
				await f.t.heldRecords.delete(await f.t.viewer(NO_SPACES), held);
				// Approved, then removed: the grant is gone, but the request was still answered.
				await answer(DJ, await ask(FRIEND), 'approve');
				await f.t.epks.revoke(await f.t.viewer(DJ), kit, FRIEND);
				const before = { states: await states(), answers: await answers() };

				await rebuild(decisions);

				expect(before.states).toEqual({ [STRANGER]: 'declined', [NO_SPACES]: 'declined', [FRIEND]: 'canAsk' });
				expect(await states()).toEqual(before.states);
				expect(before.answers.map(({ status }) => status)).toEqual(['declined', 'declined']);
				expect(await answers()).toEqual(before.answers);
				expect(await f.t.epks.waiting(f.project)).toEqual([]);
				await expect(f.t.epks.request(await f.t.viewer(STRANGER), kit, {})).rejects.toThrow(/didn't share/);
				await expect(f.t.epks.request(await f.t.viewer(NO_SPACES), kit, {})).rejects.toThrow(/didn't share/);
			}
		);

		it('gives a request that syncs after its decision back what it says, and back to its asker', async () => {
			const asker = await f.t.viewer(NO_SPACES);
			await f.t.epks.request(asker, kit, { message: 'For a May date' });
			const [{ uri }] = await f.t.epks.requests(kit.epk);
			await answer(DJ, uri, 'decline');

			await rebuild('first');

			expect(await f.t.epks.requests(kit.epk)).toMatchObject([{ uri, status: 'declined', message: 'For a May date' }]);
			expect((await f.t.heldRecords.list(asker)).map((held) => held.uri)).toEqual([uri]);
		});

		it('takes a decision only from the root, in the request space', async () => {
			const request = await ask(STRANGER);

			const upsertDecision = (inSpace: string, repoDid: string, value: JsonObject) =>
				sync({
					kind: 'upsert',
					space: inSpace,
					repoDid,
					collection: COLLECTION.epkDecision,
					rkey: '3dec',
					uri: `${inSpace}/${repoDid}/${COLLECTION.epkDecision}/3dec`,
					cid: 'bafyreiabc',
					value: { $type: COLLECTION.epkDecision, ...value }
				});

			// The request space takes public writes, so anyone could write a decision into it.
			await upsertDecision(space, STRANGER, decision(request, 'approved'));
			await upsertDecision(space, DJ, decision(request, 'declined'));
			await upsertDecision(kit.epk.space, ROOT, decision(request, 'declined'));
			await upsertDecision(space, ROOT, decision(request, 'maybe'));
			expect((await f.t.epks.accessState(kit, await f.t.viewer(STRANGER))).kind).toBe('pending');

			await upsertDecision(space, ROOT, decision(request, 'declined'));
			expect(await f.t.epks.accessState(kit, await f.t.viewer(STRANGER))).toEqual({ kind: 'declined' });
		});
	});

	it("indexes press kit drafts only from a backstage under the root, and drops them when they're deleted", async () => {
		const space = backstageSpace(ROOT, '3proj');
		const now = new Date().toISOString();
		const draft = { changes: { title: 'Clubs 2027' }, createdAt: now, updatedAt: now };
		const drafts = () => t.db.selectFrom('epkDraft').select(['authorDid', 'epkRkey']).execute();

		await upsert(backstageSpace(ALICE, '3proj'), ALICE, COLLECTION.epkDraft, '3kit', draft);
		expect(await drafts()).toEqual([]);

		await upsert(space, ALICE, COLLECTION.epkDraft, '3kit', draft);
		expect(await drafts()).toEqual([{ authorDid: ALICE, epkRkey: '3kit' }]);

		await project({
			kind: 'delete',
			space,
			repoDid: ALICE,
			collection: COLLECTION.epkDraft,
			rkey: '3kit',
			uri: `${space}/${ALICE}/${COLLECTION.epkDraft}/3kit`
		});
		expect(await drafts()).toEqual([]);
	});

	it("marks the root's audit copy failed when it's deleted", async () => {
		const entry = await t.audit.record({ actor: ALICE, action: 'auth.login', summary: 'in' }, { awaitDelivery: true });
		const stored = await t.audit.get(entry.id);
		await project({
			kind: 'delete',
			space: t.spaces.audit,
			repoDid: ROOT,
			collection: COLLECTION.auditEntry,
			rkey: entry.id,
			uri: stored!.rootUri!
		});
		expect(await t.audit.get(entry.id)).toMatchObject({
			rootStatus: 'failed',
			rootError: 'Copy was deleted from the repo',
			actorStatus: stored!.actorStatus
		});
	});

	it('ignores records of these collections outside the spaces they belong in', async () => {
		const now = new Date().toISOString();
		const p1 = 'at://did:plc:root/computer.localnetwork.project/p1';

		await upsert(t.spaces.staff, ROOT, COLLECTION.request, 'r1', {
			kind: 'create',
			profile: { name: 'A', kind: 'solo', genres: ['x'], location: { city: 'c', country: 'DE' } },
			createdAt: now
		});
		expect(await t.requests.byRkey('r1')).toBeNull();

		await upsert(t.spaces.moderation, ROOT, COLLECTION.projectDetails, 'p1', {
			project: p1,
			details: { bookingEmail: 'x@y.test' },
			updatedAt: now
		});
		await upsert(t.spaces.industry, ALICE, COLLECTION.projectDetails, 'p1', {
			project: p1,
			details: { bookingEmail: 'x@y.test' },
			updatedAt: now
		});
		expect(await t.projects.details(p1)).toBeNull();

		await upsert(t.spaces.staff, ROOT, COLLECTION.bookingMessage, 'm1', { text: 'hi', createdAt: now });
		await upsert(bookingSpace(ALICE, 'b1'), ALICE, COLLECTION.bookingMessage, 'm2', { text: 'hi', createdAt: now });
		expect(await t.db.selectFrom('bookingEvent').selectAll().execute()).toEqual([]);

		await upsert(epkSpace(ROOT, '3kit'), ALICE, COLLECTION.epkDraft, '3kit', {
			changes: { title: 'Clubs' },
			createdAt: now,
			updatedAt: now
		});
		expect(await t.db.selectFrom('epkDraft').selectAll().execute()).toEqual([]);

		// A real audit copy, as the actor wrote it, synced from the wrong space.
		const entry = await t.audit.record({ actor: ALICE, action: 'auth.login', summary: 'in' }, { awaitDelivery: true });
		const copy = t.net.spaceRecordsIn(t.spaces.audit, ALICE).find((record) => record.uri.endsWith(entry.id))!;
		await upsert(t.spaces.moderation, ALICE, COLLECTION.auditEntry, entry.id, copy.value);
		expect(await t.db.selectFrom('auditCopy').selectAll().execute()).toEqual([]);
	});

	describe("a performer's, party's or venue's space", () => {
		const space = sharedSpace(ROOT, 'party', '3party');
		const now = new Date().toISOString();

		const remove = (inSpace: string, repoDid: string, collection: string, rkey: string) =>
			project({
				kind: 'delete',
				space: inSpace,
				repoDid,
				collection,
				rkey,
				uri: `${inSpace}/${repoDid}/${collection}/${rkey}`
			});

		const person = (subject: string) => ({
			subject,
			role: 'door',
			capabilities: ['guests.checkin', 'venue.block'],
			hidden: true,
			via: 'staff',
			createdAt: now,
			updatedAt: now
		});

		const people = () => t.db.selectFrom('spacePerson').select(['did', 'role', 'capabilities', 'hidden']).execute();

		it('indexes only what the root wrote, in a space under the root, and drops it when deleted', async () => {
			await upsert(space, ALICE, COLLECTION.person, ALICE, person(ALICE));
			await upsert(sharedSpace(ALICE, 'party', '3party'), ALICE, COLLECTION.person, ALICE, person(ALICE));
			await upsert(sharedSpace(ALICE, 'party', '3party'), ROOT, COLLECTION.person, ALICE, person(ALICE));
			expect(await people()).toEqual([]);

			await upsert(space, ROOT, COLLECTION.person, ALICE, person(ALICE));
			// Keyed by someone else's DID, so it's not a person record this app wrote.
			await upsert(space, ROOT, COLLECTION.person, BOB, person(ALICE));
			expect(await people()).toEqual([
				{ did: ALICE, role: 'door', capabilities: JSON.stringify(['guests.checkin']), hidden: 1 }
			]);

			await remove(space, ALICE, COLLECTION.person, ALICE);
			expect(await people()).toHaveLength(1);
			await remove(space, ROOT, COLLECTION.person, ALICE);
			expect(await people()).toEqual([]);
		});

		it('indexes invites, invite links, requests to join and the listing status, and forgets them with the space', async () => {
			const capabilities = ['guests.checkin'];
			await upsert(space, ROOT, COLLECTION.invite, BOB, {
				subject: BOB,
				role: 'door',
				capabilities,
				invitedBy: ALICE,
				createdAt: now
			});
			await upsert(space, ROOT, COLLECTION.inviteLink, '3link', {
				tokenHash: 'hash',
				role: 'door',
				capabilities,
				createdBy: ALICE,
				createdAt: now
			});
			await upsert(space, ROOT, COLLECTION.joinRequest, BOB, { subject: BOB, message: 'Hi', createdAt: now });
			await upsert(space, ROOT, COLLECTION.listingStatus, 'self', { listed: true, createdBy: ALICE, createdAt: now });

			const tables = ['spaceInvite', 'spaceInviteLink', 'spaceJoinRequest', 'sharedSpace'] as const;

			const counts = async () =>
				Promise.all(tables.map(async (table) => (await t.db.selectFrom(table).selectAll().execute()).length));

			expect(await counts()).toEqual([1, 1, 1, 1]);
			expect(await t.sharedSpaces.get(space)).toMatchObject({ kind: 'party', listed: true, createdBy: ALICE });

			await remove(space, ROOT, COLLECTION.inviteLink, '3link');
			await remove(space, ROOT, COLLECTION.listingStatus, 'self');
			expect(await counts()).toEqual([1, 0, 1, 0]);

			await remove(space, ROOT, COLLECTION.invite, BOB);
			await remove(space, ROOT, COLLECTION.joinRequest, BOB);
			expect(await counts()).toEqual([0, 0, 0, 0]);

			await upsert(space, ROOT, COLLECTION.invite, BOB, {
				subject: BOB,
				role: 'door',
				capabilities,
				invitedBy: ALICE,
				createdAt: now
			});
			await t.sharedSpaces.forget(space);
			expect(await counts()).toEqual([0, 0, 0, 0]);
		});

		it("indexes a performer space's performer record only from the root, at its key, and drops it when deleted", async () => {
			const performerSpace = sharedSpace(ROOT, 'performer', '3perf');
			const dj = { subject: { $type: `${COLLECTION.performer}#dj`, did: ALICE }, createdAt: now, updatedAt: now };
			const rows = () => t.db.selectFrom('performer').select(['spaceUri', 'kind', 'did']).execute();

			await upsert(performerSpace, ALICE, COLLECTION.performer, 'self', dj);
			await upsert(sharedSpace(ALICE, 'performer', '3perf'), ROOT, COLLECTION.performer, 'self', dj);
			await upsert(performerSpace, ROOT, COLLECTION.performer, 'other', dj);
			// Only a performer's space says who it's for.
			await upsert(space, ROOT, COLLECTION.performer, 'self', dj);
			expect(await rows()).toEqual([]);

			await upsert(performerSpace, ROOT, COLLECTION.performer, 'self', dj);
			expect(await rows()).toEqual([{ spaceUri: performerSpace, kind: 'dj', did: ALICE }]);

			await remove(performerSpace, ALICE, COLLECTION.performer, 'self');
			expect(await rows()).toHaveLength(1);
			await remove(performerSpace, ROOT, COLLECTION.performer, 'self');
			expect(await rows()).toEqual([]);
		});

		it("indexes a venue space's venue record only from the root, at its key, and drops it when deleted", async () => {
			const venueSpace = sharedSpace(ROOT, 'venue', '3venue');
			const venue = { name: 'Tresor', address: { country: 'DE' }, createdAt: now, updatedAt: now };
			const rows = () => t.db.selectFrom('venue').select(['spaceUri', 'skey', 'nameKey']).execute();

			await upsert(venueSpace, ALICE, COLLECTION.venue, 'self', venue);
			await upsert(sharedSpace(ALICE, 'venue', '3venue'), ROOT, COLLECTION.venue, 'self', venue);
			await upsert(venueSpace, ROOT, COLLECTION.venue, 'other', venue);
			// Only a venue's space holds a venue.
			await upsert(space, ROOT, COLLECTION.venue, 'self', venue);
			await upsert(venueSpace, ROOT, COLLECTION.venue, 'self', { ...venue, address: {} });
			expect(await rows()).toEqual([]);

			await upsert(venueSpace, ROOT, COLLECTION.venue, 'self', venue);
			expect(await rows()).toEqual([{ spaceUri: venueSpace, skey: '3venue', nameKey: 'tresor' }]);

			await remove(venueSpace, ALICE, COLLECTION.venue, 'self');
			expect(await rows()).toHaveLength(1);
			await remove(venueSpace, ROOT, COLLECTION.venue, 'self');
			expect(await rows()).toEqual([]);
		});

		it('indexes events in a party space and approvals in a venue space only from the root, and drops them', async () => {
			const partySpace = sharedSpace(ROOT, 'party', '3party');
			const venueSpace = sharedSpace(ROOT, 'venue', '3venue');

			const event = {
				name: 'Winter Opening',
				startsAt: '2026-12-05T23:00:00+01:00',
				venue: venueSpace,
				status: 'community.lexicon.calendar.event#scheduled',
				audience: 'public',
				createdBy: ALICE,
				createdAt: now,
				updatedAt: now
			};

			const eventUri = `${partySpace}/${ROOT}/${COLLECTION.event}/3e1`;
			const approval = { event: eventUri, approvedBy: ALICE, createdAt: now };
			const events = () => t.db.selectFrom('event').select(['uri', 'venueSpace']).execute();
			const approvals = () => t.db.selectFrom('venueApproval').select(['spaceUri', 'eventUri']).execute();

			await upsert(partySpace, ALICE, COLLECTION.event, '3e1', event);
			await upsert(venueSpace, ROOT, COLLECTION.event, '3e1', event);
			await upsert(partySpace, ROOT, COLLECTION.event, '3e2', { ...event, startsAt: 'tonight' });
			await upsert(venueSpace, ALICE, COLLECTION.venueApproval, '3a1', approval);
			await upsert(partySpace, ROOT, COLLECTION.venueApproval, '3a1', approval);
			expect([await events(), await approvals()]).toEqual([[], []]);

			await upsert(partySpace, ROOT, COLLECTION.event, '3e1', event);
			await upsert(venueSpace, ROOT, COLLECTION.venueApproval, '3a1', approval);
			expect(await events()).toEqual([{ uri: eventUri, venueSpace }]);
			expect(await approvals()).toEqual([{ spaceUri: venueSpace, eventUri }]);

			await remove(partySpace, ROOT, COLLECTION.event, '3e1');
			await remove(venueSpace, ROOT, COLLECTION.venueApproval, '3a1');
			expect([await events(), await approvals()]).toEqual([[], []]);
		});

		it('indexes an event without a status as scheduled, the calendar status default', async () => {
			const partySpace = sharedSpace(ROOT, 'party', '3party');
			const event = { name: 'Winter Opening', audience: 'public', createdBy: ALICE, createdAt: now, updatedAt: now };

			await upsert(partySpace, ROOT, COLLECTION.event, '3e1', event);
			expect(await t.db.selectFrom('event').select('status').execute()).toEqual([
				{ status: 'community.lexicon.calendar.event#scheduled' }
			]);
		});

		it("indexes a party space's party record only from the root, at its key, and drops it when deleted", async () => {
			const partySpace = sharedSpace(ROOT, 'party', '3party');
			const party = { basics: { name: 'Night Shift' }, createdAt: now, updatedAt: now };
			const rows = () => t.db.selectFrom('party').select(['spaceUri', 'skey', 'name']).execute();

			await upsert(partySpace, ALICE, COLLECTION.party, 'self', party);
			await upsert(sharedSpace(ALICE, 'party', '3party'), ROOT, COLLECTION.party, 'self', party);
			await upsert(partySpace, ROOT, COLLECTION.party, 'other', party);
			// Only a party's space holds a party.
			await upsert(sharedSpace(ROOT, 'venue', '3venue'), ROOT, COLLECTION.party, 'self', party);
			await upsert(partySpace, ROOT, COLLECTION.party, 'self', { ...party, basics: {} });
			expect(await rows()).toEqual([]);

			await upsert(partySpace, ROOT, COLLECTION.party, 'self', party);
			expect(await rows()).toEqual([{ spaceUri: partySpace, skey: '3party', name: 'Night Shift' }]);

			await remove(partySpace, ALICE, COLLECTION.party, 'self');
			expect(await rows()).toHaveLength(1);
			await remove(partySpace, ROOT, COLLECTION.party, 'self');
			expect(await rows()).toEqual([]);
		});

		it('ignores records keyed by the wrong DID and listing statuses under any other key', async () => {
			await upsert(space, ROOT, COLLECTION.invite, ALICE, {
				subject: BOB,
				role: 'door',
				capabilities: [],
				invitedBy: ALICE,
				createdAt: now
			});
			await upsert(space, ROOT, COLLECTION.joinRequest, ALICE, { subject: BOB, createdAt: now });
			await upsert(space, ROOT, COLLECTION.listingStatus, 'other', { listed: true, createdBy: ALICE, createdAt: now });

			for (const table of ['spaceInvite', 'spaceJoinRequest', 'sharedSpace'] as const)
				expect(await t.db.selectFrom(table).selectAll().execute()).toEqual([]);
		});
	});
});
