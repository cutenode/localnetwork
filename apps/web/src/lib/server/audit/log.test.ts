import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { rootSpaces } from '../atproto/spaces';
import { type Db, openDatabase } from '../db';
import { silentLogger } from '../logger';
import { FakeNetwork } from '../testing/fakes';
import { canonicalJson, chainHash, sanitizeDetails } from './chain';
import auditEntry from '../../../../../../lexicons/computer/localnetwork/audit/entry.json' with { type: 'json' };
import { AUDIT_ACTIONS, AuditLog, SUBJECT_KINDS } from './log';

const ROOT = 'did:plc:root';

const ALICE = 'did:plc:alice';

const BOB = 'did:plc:bob';

const space = rootSpaces(ROOT).audit;

describe('the audit entry lexicon', () => {
	const { properties } = auditEntry.defs.main.record;

	it('lists every action and subject kind the log records, so other readers know them', () => {
		expect(properties.action.knownValues).toEqual([...AUDIT_ACTIONS]);
		expect(properties.subjectKind.knownValues).toEqual([...SUBJECT_KINDS]);
	});
});

describe('AuditLog', () => {
	let db: Db;
	let net: FakeNetwork;
	let log: AuditLog;
	let capable: Set<string>;
	let sessions: Set<string>;

	beforeEach(async () => {
		db = await openDatabase(':memory:');
		net = new FakeNetwork();
		capable = new Set([ALICE]);
		sessions = new Set([ALICE]);
		log = new AuditLog({
			db,
			logger: silentLogger,
			auditSpace: () => space,
			rootDid: () => ROOT,
			rootClient: async () => net.client(ROOT),
			userClient: async (did) => (sessions.has(did) ? net.client(did) : null),
			spacesCapable: async (did) => capable.has(did),
			maxAttempts: 3
		});
	});
	afterEach(() => db.destroy());

	it('writes the actor copy and a chained root copy with the same record key', async () => {
		const entry = await log.record(
			{
				actor: ALICE,
				action: 'auth.login',
				subject: ALICE,
				subjectKind: 'account',
				summary: 'Signed in',
				details: { ratio: 0.5 }
			},
			{ awaitDelivery: true }
		);

		const stored = await log.get(entry.id);
		expect(stored?.rootStatus).toBe('written');
		expect(stored?.actorStatus).toBe('written');
		expect(stored?.nextAttemptAt).toBeNull();

		const rootCopy = await net
			.client(ROOT)
			.getSpaceRecord({ space, repo: ROOT, collection: COLLECTION.auditEntry, rkey: entry.id });

		const aliceCopy = await net
			.client(ROOT)
			.getSpaceRecord({ space, repo: ALICE, collection: COLLECTION.auditEntry, rkey: entry.id });

		expect(rootCopy?.value.chain).toEqual({ seq: 1, prev: '', hash: entry.hash });
		expect(aliceCopy?.value.chain).toBeUndefined();
		expect(aliceCopy?.value).toMatchObject({
			actor: ALICE,
			action: 'auth.login',
			summary: 'Signed in',
			details: { ratio: '0.5' }
		});
		expect(rootCopy?.value.$type).toBe(COLLECTION.auditEntry);
	});

	it('chains entries and verifies the chain', async () => {
		for (let i = 0; i < 5; i++) {
			await log.record({ actor: ALICE, action: 'planner.save', summary: `save ${i}` }, { awaitDelivery: true });
		}

		const entries = await log.list();
		expect(entries.map((e) => e.seq)).toEqual([5, 4, 3, 2, 1]);
		expect(entries[0].prevHash).toBe(entries[1].hash);
		expect(await log.verifyChain()).toEqual({ ok: true, checked: 5 });

		await db.updateTable('auditEntry').set({ summary: 'tampered' }).where('seq', '=', 3).execute();
		expect(await log.verifyChain()).toMatchObject({ ok: false, brokenAt: 3, problem: 'content changed' });
	});

	it('detects deleted entries', async () => {
		for (let i = 0; i < 3; i++) await log.record({ actor: ALICE, action: 'planner.save', summary: `s${i}` });
		await db.deleteFrom('auditEntry').where('seq', '=', 2).execute();
		expect(await log.verifyChain()).toMatchObject({ ok: false, brokenAt: 2, problem: 'missing entry' });
	});

	it('serializes concurrent appends into a gapless chain', async () => {
		await Promise.all(
			Array.from({ length: 20 }, (_, i) => log.record({ actor: ROOT, action: 'system.space', summary: `n${i}` }))
		);
		expect((await log.verifyChain()).checked).toBe(20);
	});

	it('marks root-actor copies as self and non-spaces accounts as unsupported', async () => {
		const own = await log.record({ actor: ROOT, action: 'system.bootstrap', summary: 'boot' }, { awaitDelivery: true });
		expect((await log.get(own.id))?.actorStatus).toBe('self');
		const legacy = await log.record({ actor: BOB, action: 'auth.login', summary: 'hi' }, { awaitDelivery: true });
		const stored = await log.get(legacy.id);
		expect(stored?.actorStatus).toBe('unsupported');
		expect(stored?.rootStatus).toBe('written');
	});

	it('retries failed copies with backoff and gives up after max attempts', async () => {
		capable.add(BOB);
		net.failWrites(ROOT, 1);

		const entry = await log.record(
			{ actor: BOB, action: 'auth.login', summary: 'no session' },
			{ awaitDelivery: true }
		);

		let stored = await log.get(entry.id);
		expect(stored?.rootStatus).toBe('pending');
		expect(stored?.rootAttempts).toBe(1);
		expect(stored?.actorStatus).toBe('pending');
		expect(stored?.actorError).toMatch(/No active session/);
		expect(stored?.nextAttemptAt).not.toBeNull();

		await db
			.updateTable('auditEntry')
			.set({ nextAttemptAt: new Date(0).toISOString() })
			.execute();
		expect(await log.processPending()).toBe(1);
		stored = await log.get(entry.id);
		expect(stored?.rootStatus).toBe('written');

		for (let i = 0; i < 3; i++) {
			await db
				.updateTable('auditEntry')
				.set({ nextAttemptAt: new Date(0).toISOString() })
				.execute();
			await log.processPending();
		}

		stored = await log.get(entry.id);
		expect(stored?.actorStatus).toBe('failed');
		expect(stored?.nextAttemptAt).toBeNull();
		expect(await log.stats()).toMatchObject({ total: 1, failed: 1, pending: 0 });
	});

	it('treats an already-written copy as success (idempotent redelivery)', async () => {
		const entry = await log.record({ actor: ALICE, action: 'auth.login', summary: 'x' }, { awaitDelivery: true });
		await db
			.updateTable('auditEntry')
			.set({ rootStatus: 'pending', actorStatus: 'pending' })
			.where('id', '=', entry.id)
			.execute();
		await log.deliver(entry.id);
		const stored = await log.get(entry.id);
		expect(stored?.rootStatus).toBe('written');
		expect(stored?.actorStatus).toBe('written');
	});

	it('verifies copies found by sync and rebuilds lost entries from the root chain', async () => {
		const entry = await log.record(
			{ actor: ALICE, action: 'request.submit', summary: 'filed', changes: [{ field: 'name', after: 'X' }] },
			{ awaitDelivery: true }
		);

		for (const repo of [ROOT, ALICE]) {
			const copy = await net
				.client(ROOT)
				.getSpaceRecord({ space, repo, collection: COLLECTION.auditEntry, rkey: entry.id });

			await log.onSyncedCopy({ uri: copy!.uri, repoDid: repo, rkey: entry.id, cid: copy!.cid, value: copy!.value });
		}

		const stored = await log.get(entry.id);
		expect(stored?.rootStatus).toBe('verified');
		expect(stored?.actorStatus).toBe('verified');

		await db.deleteFrom('auditEntry').execute();

		const copy = await net
			.client(ROOT)
			.getSpaceRecord({ space, repo: ROOT, collection: COLLECTION.auditEntry, rkey: entry.id });

		await log.onSyncedCopy({ uri: copy!.uri, repoDid: ROOT, rkey: entry.id, cid: copy!.cid, value: copy!.value });
		expect(await log.verifyChain()).toEqual({ ok: true, checked: 1 });
	});

	it('reports orphan copies that have no root entry', async () => {
		await log.onSyncedCopy({ uri: 'at://x', repoDid: BOB, rkey: 'zzz', cid: 'c', value: { actor: BOB } });
		expect((await log.orphans()).map((o) => o.repoDid)).toEqual([BOB]);
	});

	it('filters the log', async () => {
		await log.record({ actor: ALICE, action: 'auth.login', summary: 'Alice in' });
		await log.record({
			actor: BOB,
			action: 'request.submit',
			summary: 'Bob filed a claim',
			subject: 'at://p',
			outcome: 'denied'
		});
		expect((await log.list({ actor: BOB })).length).toBe(1);
		expect((await log.list({ action: 'auth' })).length).toBe(1);
		expect((await log.list({ query: 'claim' })).length).toBe(1);
		expect((await log.list({ outcome: 'denied' })).length).toBe(1);
		expect((await log.list({ subject: 'at://p' })).length).toBe(1);
		expect((await log.list({ before: 2 })).length).toBe(1);
	});
});

describe('chain helpers', () => {
	it('produces canonical JSON regardless of key order', () => {
		expect(canonicalJson({ b: 1, a: { d: 2, c: [3, { f: 1, e: 0 }] }, z: undefined })).toBe(
			'{"a":{"c":[3,{"e":0,"f":1}],"d":2},"b":1}'
		);
	});

	it('hashes deterministically and ignores null fields', () => {
		const base = {
			id: '1',
			seq: 1,
			actor: 'a',
			action: 'x',
			summary: 's',
			outcome: 'success',
			via: 'web',
			createdAt: 't'
		};

		expect(chainHash({ ...base, reason: null }, '')).toBe(chainHash(base, ''));
		expect(chainHash(base, 'p')).not.toBe(chainHash(base, ''));
	});

	it('keeps the hash of an existing entry byte-for-byte stable', () => {
		const content = {
			id: '3l4abcxyz2k2a',
			seq: 7,
			actor: 'did:plc:alice',
			actorRole: null,
			action: 'request.submit',
			subject: 'at://did:plc:alice/computer.localnetwork.moderation.request/3l4',
			subjectKind: 'request',
			summary: 'Filed a claim for “Neon Tide”',
			changes: [
				{ field: 'name', after: 'Neon Tide' },
				{ field: 'genres', before: 'house', after: 'house, techno' }
			],
			details: { ratio: '0.5', admins: ['did:plc:a', 'did:plc:b'], nested: { z: 1, a: null, m: [true, null] } },
			outcome: 'success',
			reason: null,
			via: 'web',
			createdAt: '2026-01-02T03:04:05.678Z'
		};

		expect(chainHash(content, 'f'.repeat(64))).toBe('45ad25f300c5ada1eb27321d629360666b9e314e6343fc535c92584a31a095f5');
		expect(chainHash({ ...content, changes: undefined, details: undefined }, '')).toBe(
			'2c1b5249dbe613aa6e48390f375e15b36da67e96c57228b6741060e457a95b04'
		);
	});

	it('sanitizes details for the atproto data model', () => {
		expect(
			sanitizeDetails({
				f: 1.5,
				i: 2,
				b: true,
				n: null,
				u: undefined,
				d: new Date(0),
				big: 10n,
				s: 'x'.repeat(5000),
				arr: [1, undefined]
			})
		).toMatchObject({ f: '1.5', i: 2, b: true, d: '1970-01-01T00:00:00.000Z', big: '10', arr: [1, null] });
		expect(sanitizeDetails({ a: { b: { c: { d: { e: { f: { g: 1 } } } } } } })).toEqual({
			a: { b: { c: { d: { e: { f: '[truncated]' } } } } }
		});
	});
});
