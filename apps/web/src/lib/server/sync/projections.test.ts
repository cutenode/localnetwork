import type { JsonObject } from '@atproto/lex-json';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { bookingSpace, epkSpace } from '../atproto/spaces';
import { createTestServices, ROOT } from '../testing/core';
import { createProjections } from './projections';

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
			bookings: t.bookings,
			epks: t.epks
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

	it("counts a press kit grant only from the press kit's owner, and drops it when deleted", async () => {
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
});
