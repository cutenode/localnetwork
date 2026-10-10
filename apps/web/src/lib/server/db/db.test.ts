import { sql } from 'kysely';
import { afterEach, describe, expect, it } from 'vitest';
import { type Db, openDatabase, parseJson } from './index';

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
