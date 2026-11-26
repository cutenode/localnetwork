import { CamelCasePlugin, Kysely, sql, SqliteDialect } from 'kysely';
import { Migrator } from 'kysely/migration';
import { afterEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { writtenAs } from '../atproto/spaces';
import { type Db, nowIso, openDatabase, parseJson } from './index';
import { migrations } from './migrations';
import { type Database, SERVER_ONLY_COLUMNS, TABLE_SOURCE } from './schema';
import { NodeSqliteDatabase } from './sqlite';

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
		const space = 'at:space/did:plc:root/computer.localnetwork.space.booking/gig';

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

	describe('rewriting space URIs from the alpha', () => {
		const ROOT = 'did:plc:root';
		const space = `at:space/${ROOT}/computer.localnetwork.space.booking/3gig`;
		const uri = `${space}/${ROOT}/${COLLECTION.bookingStatus}/3s`;
		const alpha = (value: string) => writtenAs(value)[1];
		const at = '2026-11-01T00:00:00.000Z';

		/** Every value in the tables the alpha's rows are in, and what each should become. */
		const rows = (uriOf: (value: string) => string) => ({
			spaceWatch: {
				spaceUri: uriOf(space),
				spaceType: 'computer.localnetwork.space.booking',
				authorityDid: ROOT,
				skey: '3gig',
				spaceRev: null,
				registrationExpiresAt: null,
				notifyToken: 'token',
				lastSyncedAt: at,
				lastError: null,
				createdAt: at
			},
			bookingThread: {
				spaceUri: uriOf(space),
				skey: '3gig',
				projectUri: `at://${ROOT}/${COLLECTION.project}/3p`,
				promoterDid: 'did:plc:promoter',
				status: 'open',
				title: null,
				showDate: null,
				createdAt: at,
				updatedAt: at,
				lastActivityAt: at
			},
			bookingParticipant: { spaceUri: uriOf(space), did: 'did:plc:promoter', role: 'promoter' as const },
			bookingEvent: {
				uri: uriOf(uri),
				spaceUri: uriOf(space),
				authorDid: ROOT,
				kind: 'status' as const,
				record: JSON.stringify({ plannerShow: uriOf(uri), text: 'at the Door', list: [uriOf(space)] }),
				createdAt: at
			},
			notification: {
				id: 'n',
				recipientDid: 'did:plc:promoter',
				kind: 'booking.status',
				dedupeKey: uriOf(uri),
				subjectUri: uriOf(space),
				href: '/dashboard/bookings/3gig',
				data: JSON.stringify({ skey: '3gig' }),
				occurredAt: at,
				createdAt: at,
				readAt: null
			},
			notificationDelivery: {
				id: 'd',
				notificationId: 'n',
				did: 'did:plc:promoter',
				channel: 'email' as const,
				status: 'pending' as const,
				batchId: null,
				attempts: 0,
				nextAttemptAt: at,
				lastError: null,
				providerId: null,
				sentAt: null,
				updatedAt: at
			},
			venueBlock: {
				uri: `at://${ROOT}/${COLLECTION.venueBlock}/3b`,
				venue: uriOf(space),
				subject: `at://${ROOT}/${COLLECTION.listing}/3l`,
				createdAt: at
			}
		});

		/** A database as the migration finds it, whatever later ones change. */
		const before = async (): Promise<Db> => {
			const database = new Kysely<Database>({
				dialect: new SqliteDialect({ database: new NodeSqliteDatabase(':memory:') }),
				plugins: [new CamelCasePlugin()]
			});

			const migrator = new Migrator({ db: database, provider: { getMigrations: async () => migrations } });
			const { error } = await migrator.migrateTo('023_feature_flag_signed_out');

			if (error) throw error;

			return database;
		};

		const seed = async (database: Db, uriOf: (value: string) => string) => {
			for (const [table, row] of Object.entries(rows(uriOf)))
				// SAFETY: each key of rows() is a table, and its value one of that table's rows.
				await database
					.insertInto(table as 'kv')
					.values(row as never)
					.execute();
			await database
				.insertInto('kv')
				.values({ key: `listing_digest:${uriOf(space)}`, value: 'digest' })
				.execute();
		};

		const dump = (database: Db) =>
			Promise.all(
				[...Object.keys(rows(String)), 'kv'].map(
					async (table) => (await sql`SELECT * FROM ${sql.table(table)}`.execute(database)).rows
				)
			);

		it('rewrites them wherever they are, and the JSON that holds them, keeping the rest', async () => {
			db = await before();
			await seed(db, alpha);
			const expected = await before();
			await seed(expected, (value) => value);

			await migrations['024_space_uris'].up(db);

			expect(await dump(db)).toEqual(await dump(expected));
			expect((await sql`PRAGMA foreign_key_check`.execute(db)).rows).toEqual([]);
			await expected.destroy();
		});

		it('changes nothing when it runs again', async () => {
			db = await before();
			await seed(db, alpha);
			await migrations['024_space_uris'].up(db);
			const first = await dump(db);

			await migrations['024_space_uris'].up(db);

			expect(await dump(db)).toEqual(first);
		});

		it('rewrites every column that can hold one, but what a CID or an audit hash covers', async () => {
			db = await before();
			const URI = /^(uri|.*_uri|venue|venue_space|source|dedupe_key)$/;
			const JSON_COLUMN = /^(record|data)$/;
			const PUBLIC = /project_uri$|epk_uri$|^venue_block\.uri$/;

			const { rows: columns } = await sql<{ table: string; name: string; type: string }>`
				SELECT m.name AS "table", c.name, c.type FROM sqlite_master m, pragma_table_info(m.name) c
				WHERE m.type = 'table' AND m.name NOT GLOB 'sqlite_*' AND m.name NOT GLOB 'kysely_*' AND m.name <> 'kv'
			`.execute(db);

			const value = ({ name, type }: { name: string; type: string }) => {
				if (type !== 'TEXT') return type === 'BLOB' ? new Uint8Array([1]) : 1;

				return JSON_COLUMN.test(name) ? JSON.stringify({ at: alpha(uri) }) : alpha(uri);
			};

			await sql`PRAGMA foreign_keys = OFF`.execute(db);

			for (const [table, cells] of Map.groupBy(columns, (column) => column.table))
				await sql`
					INSERT INTO ${sql.table(table)} (${sql.join(cells.map(({ name }) => sql.ref(name)))})
					VALUES (${sql.join(cells.map(value))})
				`.execute(db);

			await migrations['024_space_uris'].up(db);
			const left: string[] = [];

			for (const { table, name } of columns.filter(
				(column) => URI.test(column.name) || JSON_COLUMN.test(column.name)
			)) {
				const { rows: found } = await sql<{ text: string }>`
					SELECT ${sql.ref(name)} AS text FROM ${sql.table(table)}
				`.execute(db);

				if (found.some(({ text }) => text.includes('/space/'))) left.push(`${table}.${name}`);
			}

			expect(left.filter((column) => !PUBLIC.test(column))).toEqual(['audit_copy.record']);
		});
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
