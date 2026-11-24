import { sql } from 'kysely';
import { afterEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { type Db, nowIso, openDatabase, parseJson } from './index';
import { migrations } from './migrations';
import { SERVER_ONLY_COLUMNS, TABLE_SOURCE } from './schema';

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

		// Run on its own, as the migrator can't run it again once later migrations have.
		await sql`DROP TABLE pending_projection`.execute(db);
		await migrations['013_pending_projection'].up(db);

		const queued = await db.selectFrom('pendingProjection').selectAll().orderBy('collection').execute();
		expect(queued.map(({ collection, rkey }) => [collection, rkey])).toEqual([
			[COLLECTION.bookingInquiry, 'lost'],
			[COLLECTION.epkGrant, 'lost'],
			[COLLECTION.epkRequest, 'lost']
		]);
	});

	// Backups keep what a re-sync can't bring back, so every table has to say which it is.
	it('marks every table rebuilt or server-only, and names only columns that exist', async () => {
		db = await openDatabase(':memory:');

		const camel = (name: string) => name.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase());
		const snake = (name: string) => name.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);

		const columns = async (table: string) =>
			(await sql<{ name: string }>`PRAGMA table_info(${sql.raw(snake(table))})`.execute(db!)).rows.map(
				({ name }) => name
			);

		const tables = await sql<{ name: string }>`
			SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT GLOB 'sqlite_*' AND name NOT GLOB 'kysely_*'
		`.execute(db);

		expect(tables.rows.map(({ name }) => camel(name)).toSorted()).toEqual(Object.keys(TABLE_SOURCE).toSorted());

		for (const [table, named] of Object.entries(SERVER_ONLY_COLUMNS))
			expect(await columns(table)).toEqual(expect.arrayContaining(named.map(snake)));
	});

	it("drops the account's display name and avatar columns, keeping every account", async () => {
		db = await openDatabase(':memory:');

		const columns = async () =>
			(await sql<{ name: string }>`PRAGMA table_info(account)`.execute(db!)).rows.map(({ name }) => name);

		expect(await columns()).not.toContain('display_name');
		expect(await columns()).not.toContain('avatar_url');

		// Put the table back as 021 left it, with an account in it, and run 022 on its own.
		await sql`ALTER TABLE account ADD COLUMN display_name TEXT`.execute(db);
		await sql`ALTER TABLE account ADD COLUMN avatar_url TEXT`.execute(db);
		await sql`INSERT INTO account (did, handle, created_at, updated_at) VALUES ('did:plc:a', 'a.test', 'now', 'now')`.execute(
			db
		);
		await migrations['022_account_without_profile'].up(db);

		expect(await columns()).not.toContain('display_name');
		expect(await columns()).not.toContain('avatar_url');
		expect(await db.selectFrom('account').select(['did', 'handle']).execute()).toEqual([
			{ did: 'did:plc:a', handle: 'a.test' }
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
