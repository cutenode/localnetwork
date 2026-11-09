import { sql } from 'kysely';
import { afterEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { type Db, nowIso, openDatabase, parseJson } from './index';
import { migrateToLatest } from './migrations';

describe('database', () => {
	let db: Db | undefined;
	afterEach(async () => {
		await db?.destroy();
		db = undefined;
	});

	it('migrates and round-trips rows through node:sqlite', async () => {
		db = await openDatabase(':memory:');
		await db.insertInto('kv').values({ key: 'a', value: 'b' }).execute();
		const row = await db.selectFrom('kv').selectAll().where('key', '=', 'a').executeTakeFirstOrThrow();
		expect(row).toEqual({ key: 'a', value: 'b' });
	});

	it('queues stored records a failed projection left out of their tables, once', async () => {
		db = await openDatabase(':memory:');
		const space = 'ats://did:plc:root/computer.localnetwork.space.booking/gig';

		const stored = (collection: string, rkey: string) => ({
			spaceUri: space,
			repoDid: 'did:plc:a',
			collection,
			rkey,
			cid: 'bafy',
			value: '{}',
			indexedAt: nowIso()
		});

		await db
			.insertInto('spaceRecord')
			.values([
				stored(COLLECTION.bookingInquiry, 'lost'),
				stored(COLLECTION.bookingInquiry, 'indexed'),
				stored(COLLECTION.epkRequest, 'lost'),
				stored(COLLECTION.epkGrant, 'lost'),
				stored(COLLECTION.person, 'other')
			])
			.execute();
		await db
			.insertInto('bookingEvent')
			.values({
				uri: `${space}/did:plc:a/${COLLECTION.bookingInquiry}/indexed`,
				spaceUri: space,
				authorDid: 'did:plc:a',
				kind: 'inquiry',
				record: '{}',
				createdAt: nowIso()
			})
			.execute();

		await sql`DROP TABLE pending_projection`.execute(db);
		await sql`DELETE FROM kysely_migration WHERE name = '013_pending_projection'`.execute(db);
		await migrateToLatest(db);

		const queued = await db.selectFrom('pendingProjection').selectAll().orderBy('collection').execute();
		expect(queued.map(({ collection, rkey }) => [collection, rkey])).toEqual([
			[COLLECTION.bookingInquiry, 'lost'],
			[COLLECTION.epkGrant, 'lost'],
			[COLLECTION.epkRequest, 'lost']
		]);
	});

	it('maps camelCase columns, binary values, and booleans', async () => {
		db = await openDatabase(':memory:');
		const lt = new Uint8Array([1, 2, 3]);
		await db
			.insertInto('syncedRepo')
			.values({
				spaceUri: 's',
				repoDid: 'did:plc:a',
				pdsUrl: 'http://x',
				rev: '1',
				ltHash: lt,
				commitHash: null,
				lastSyncedAt: 'now',
				lastError: null
			})
			.execute();
		const row = await db.selectFrom('syncedRepo').selectAll().executeTakeFirstOrThrow();
		expect(Buffer.from(row.ltHash)).toEqual(Buffer.from(lt));
		await db
			.insertInto('account')
			.values({
				did: 'did:plc:a',
				handle: null,
				displayName: null,
				avatarUrl: null,
				pdsUrl: null,
				// A boolean bound through the adapter is stored as 1.
				spacesCapable: sql<number>`${true}`,
				grantedScope: null,
				createdAt: 'now',
				updatedAt: 'now',
				lastLoginAt: null
			})
			.execute();
		const account = await db.selectFrom('account').select('spacesCapable').executeTakeFirstOrThrow();
		expect(account.spacesCapable).toBe(1);
	});

	it('parses JSON defensively', () => {
		expect(parseJson('[1]', [])).toEqual([1]);
		expect(parseJson('nope', 'fallback')).toBe('fallback');
		expect(parseJson(null, 0)).toBe(0);
	});
});
