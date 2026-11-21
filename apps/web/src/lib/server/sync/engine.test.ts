import { encode } from '@atproto/lex-cbor';
import type { LexMap } from '@atproto/lex-data';
import { type JsonObject, type JsonValue, jsonToLex, lexToJson } from '@atproto/lex-json';
import { RepoCommit } from '@atproto/space';
import * as fc from 'fast-check';
import { SPACE_TYPE } from '#lib/nsid.ts';
import { afterEach, beforeEach, describe, expect, it, onTestFinished, vi } from 'vitest';
import { SpaceCredentialManager, XrpcResponseError } from '../atproto/credentials';
import { IdentityResolver } from '../atproto/identity';
import { RecordValidationError, TOO_DEEP, validateRecord } from '../atproto/lexicons';
import { bookingSpace } from '../atproto/spaces';
import { type Db, nowIso, openDatabase } from '../db';
import { type Logger, silentLogger } from '../logger';
import { anyRecord, RECORD_COLLECTIONS, recordKey, validRecord } from '../testing/lexicon-arbitraries';
import { FakeSpaceHost } from '../testing/space-host';
import { writtenLog } from '../testing/written-log';
import { type ChangeHandler, readOpsPage, type SyncedChange, SpaceSyncer } from './engine';

const SPACE = bookingSpace('did:plc:root', 'gig');

// Stands in for arrays nested this deep, written into a listRepoOps page's text: JSON.stringify overflows the stack on
// them long before JSON.parse does.
const nested = (levels: number) => `\u0000nested:${levels}`;

const NESTED = /"\\u0000nested:(\d+)"/g;

/** A listRepoOps page as its text, with the arrays `nested` stands in for written out. */
const write = (page: JsonValue) =>
	JSON.stringify(page).replace(
		NESTED,
		(_, levels: string) => `${'['.repeat(Number(levels))}0${']'.repeat(Number(levels))}`
	);

/** Anything a repo could hold, or a valid record with arrays nested around the depth limit or far past the stack. */
const opValue = (collection: string) =>
	fc.oneof(
		anyRecord(collection),
		fc
			.tuple(
				validRecord(collection),
				fc.oneof(fc.integer({ min: 28, max: 36 }), fc.integer({ min: 1_000, max: 30_000 }))
			)
			.map(([record, levels]): JsonObject => ({ ...record, nested: nested(levels) }))
	);

/** Whether validateRecord refuses `value` as too deep. It throws nothing but RecordValidationError, which this rethrows. */
function isTooDeepFor(collection: string, value: JsonObject): boolean {
	try {
		validateRecord(collection, value);

		return false;
	} catch (error) {
		if (!(error instanceof RecordValidationError)) throw error;

		return error.message === `Invalid ${collection} record: ${TOO_DEEP}`;
	}
}

/** A listRepoOps page as JSON. */
type OpsPageJson = JsonObject & { ops: JsonObject[]; commit: JsonObject };

/** The fields of a listRepoOps page, each with what holds it: the page, its commit, or its first op. */
const FIELDS = [
	['page', 'ops'],
	['page', 'commit'],
	['page', 'cursor'],
	['commit', 'rev'],
	['commit', 'data'],
	['op', 'collection'],
	['op', 'rkey'],
	['op', 'cid'],
	['op', 'prev'],
	['op', 'value']
] as const;

// SAFETY: lexToJson only rewrites the CIDs and bytes inside a map, so a LexMap always comes back as a JSON object.
const asJson = (value: LexMap) => lexToJson(value) as JsonObject;

/** A copy of `page` with `field` set to `value`, or taken out without one, where the page has it. */
function withFieldSet(page: OpsPageJson, [holder, key]: (typeof FIELDS)[number], value: JsonValue | undefined) {
	const copy: OpsPageJson = JSON.parse(JSON.stringify(page));
	const fields = holder === 'page' ? copy : holder === 'commit' ? copy.commit : copy.ops.at(0);

	if (!fields) return copy;

	if (value === undefined) delete fields[key];
	else fields[key] = value;

	return copy;
}

describe('SpaceSyncer', () => {
	let db: Db;
	let passes: number;
	let changes: SyncedChange[];
	let dropped: string[];

	beforeEach(async () => {
		db = await openDatabase(':memory:');
		passes = 0;
		changes = [];
		dropped = [];
	});
	afterEach(() => db.destroy());

	// Every pass resolves the space authority first, so `passes` counts passes. `listWritersError`
	// makes the space host's listWriters fail; otherwise it reports no repos, or what `options.listWriters` answers.
	const syncer = (
		listWritersError?: Error | string,
		options: { logger?: Logger; listWriters?: () => Promise<object> } = {}
	) => {
		const identity = new IdentityResolver({
			plcUrl: 'https://plc.test',
			allowPrivateNetwork: false,
			logger: silentLogger
		});

		vi.spyOn(identity, 'resolve').mockImplementation(async (did) => {
			passes++;

			return {
				did,
				handle: null,
				pds: 'https://pds.test',
				signingKey: '',
				kemKey: null
			};
		});

		const credentials = new SpaceCredentialManager({
			source: async () => ({ getDelegationToken: async () => 'token' }),
			identity,
			logger: silentLogger
		});

		const listWriters = vi.spyOn(credentials, 'query');

		if (listWritersError) listWriters.mockRejectedValue(listWritersError);
		else if (options.listWriters) listWriters.mockImplementation(options.listWriters);
		else listWriters.mockResolvedValue({ repos: [] });

		return new SpaceSyncer({
			db,
			logger: options.logger ?? silentLogger,
			notifyEndpoint: null,
			onChange: async (change) => void changes.push(change),
			onSpaceDropped: async (space) => void dropped.push(space),
			identity,
			credentials
		});
	};

	const watchRow = () => db.selectFrom('spaceWatch').selectAll().where('spaceUri', '=', SPACE).executeTakeFirst();

	it('coalesces concurrent syncs of a space into one running pass and one queued pass', async () => {
		const s = syncer();
		await s.watch(SPACE);
		await Promise.all([s.sync(SPACE), s.sync(SPACE), s.sync(SPACE)]);
		expect(passes).toBe(2);
		expect((await watchRow())?.lastSyncedAt).toBeTruthy();
	});

	it('records the error on the watch and rethrows when a pass fails', async () => {
		const s = syncer(new Error('host unreachable'));

		await s.watch(SPACE);
		await expect(s.sync(SPACE)).rejects.toThrow('host unreachable');
		expect((await watchRow())?.lastError).toBe('host unreachable');
	});

	it('drops the local copy when the host says the space is gone', async () => {
		const s = syncer(new XrpcResponseError(400, 'SpaceNotFound', undefined, 'com.atproto.space.listWriters'));

		await s.watch(SPACE);
		await db
			.insertInto('spaceRecord')
			.values({
				spaceUri: SPACE,
				repoDid: 'did:plc:dj',
				collection: 'app.test.thing',
				rkey: 'a',
				cid: 'bafyreiabc',
				value: '{}',
				indexedAt: nowIso()
			})
			.execute();
		await s.sync(SPACE);
		expect(changes).toEqual([
			{
				kind: 'delete',
				space: SPACE,
				repoDid: 'did:plc:dj',
				collection: 'app.test.thing',
				rkey: 'a',
				uri: `${SPACE}/did:plc:dj/app.test.thing/a`
			}
		]);
		expect(await watchRow()).toBeUndefined();
		expect(await db.selectFrom('spaceRecord').selectAll().execute()).toEqual([]);
		expect(dropped).toEqual([SPACE]);
	});

	it('refuses to watch anything but a space', async () => {
		await expect(syncer().watch('at://did:plc:root/app.bsky.feed.post/3k')).rejects.toThrow('Not a space URI');
	});

	it('syncs every watched space, keeping each failure on its watch, and forgets an unwatched one', async () => {
		const other = bookingSpace('did:plc:root', 'other');
		const s = syncer(new Error('host unreachable'));
		await s.watch(SPACE);
		await s.watch(other);

		await s.syncAll();
		expect(passes).toBe(2);
		expect((await s.status()).map((status) => [status.space, status.type, status.lastError, status.repos])).toEqual([
			[SPACE, SPACE_TYPE.booking, 'host unreachable', []],
			[other, SPACE_TYPE.booking, 'host unreachable', []]
		]);

		await s.unwatch(SPACE);
		await s.sync(SPACE);
		expect(passes).toBe(2);
		expect((await s.status()).map((status) => status.space)).toEqual([other]);
	});

	it('keeps a failure that is not an Error as its text', async () => {
		const s = syncer('connection reset');
		await s.watch(SPACE);
		await expect(s.sync(SPACE)).rejects.toBe('connection reset');
		expect((await watchRow())?.lastError).toBe('connection reset');
	});

	it('runs the queued pass after a failing one, and warns about a failure only until it repeats', async () => {
		const { logger, lines } = writtenLog();
		const s = syncer(new Error('host unreachable'), { logger });
		await s.watch(SPACE);

		const results = await Promise.allSettled([s.sync(SPACE), s.sync(SPACE)]);
		expect(results.map((result) => result.status)).toEqual(['rejected', 'rejected']);
		expect(passes).toBe(2);
		expect(lines.map(({ level, message, space }) => [level, message, space])).toEqual([
			['warn', 'space sync failed', SPACE],
			['debug', 'space sync still failing', SPACE]
		]);
	});

	it('reads 100 pages of a repo list a pass unless told otherwise', async () => {
		const { logger, lines } = writtenLog();
		let pages = 0;
		// Every page lists a repo the host has removed, which a pass reads past, and points at the next.
		const removed = { $type: 'com.atproto.space.defs#repoRemoved', repo: 'did:plc:gone', spaceRev: '0' };
		const s = syncer(undefined, { logger, listWriters: async () => ({ repos: [removed], cursor: String(++pages) }) });
		await s.watch(SPACE);

		await s.sync(SPACE);
		expect(pages).toBe(100);
		// The pass is a job, so its warning is a note on the job's line.
		const notes = lines.flatMap(({ notes = [] }) => notes).filter(({ level }) => level === 'warn');
		expect(notes.map(({ message, pages }) => [message, pages])).toEqual([
			['repo list ran past the page limit; reading on next pass', 100]
		]);
		expect((await watchRow())?.spaceRev).toBe('100');
	});
});

describe('SpaceSyncer against a repo host', () => {
	const DJ = 'did:plc:dj';
	const NOTE = 'computer.localnetwork.test.note';
	const note = (text: string) => ({ $type: NOTE, text });

	let db: Db;
	let host: FakeSpaceHost;
	let changes: SyncedChange[];
	let dropped: string[];

	beforeEach(async () => {
		db = await openDatabase(':memory:');
		host = await FakeSpaceHost.create(SPACE);
		changes = [];
		dropped = [];
	});
	afterEach(() => db.destroy());

	const syncer = async (
		onChange: ChangeHandler = async (change) => void changes.push(change),
		notifyEndpoint: string | null = null,
		options: { maxPages?: number; maxRepoBytes?: number; logger?: Logger } = {}
	) => {
		const credentials = new SpaceCredentialManager({
			source: async () => ({ getDelegationToken: async () => 'token' }),
			identity: host.identity,
			logger: silentLogger,
			notify: (space) => s.registration(space)
		});

		const s: SpaceSyncer = new SpaceSyncer({
			db,
			logger: options.logger ?? silentLogger,
			notifyEndpoint,
			onChange,
			onSpaceDropped: async (space) => void dropped.push(space),
			identity: host.identity,
			credentials,
			maxPages: options.maxPages,
			maxRepoBytes: options.maxRepoBytes
		});

		await s.watch(SPACE);

		return s;
	};

	const records = async () =>
		(await db.selectFrom('spaceRecord').select(['rkey', 'value']).orderBy('rkey').execute()).map(({ rkey, value }) => [
			rkey,
			JSON.parse(value).text
		]);

	const repoRow = () => db.selectFrom('syncedRepo').selectAll().where('repoDid', '=', DJ).executeTakeFirst();
	const calls = (nsid: string) => host.requests.filter((request) => request.nsid === `com.atproto.space.${nsid}`);

	it('downloads a repo in full, then follows its oplog, checking each commit against its MAC', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		expect(await records()).toEqual([['a', 'one']]);
		expect(calls('getRepo')).toHaveLength(1);

		host.pageSize = 1;
		await host.put(DJ, NOTE, 'b', note('two'));
		await host.put(DJ, NOTE, 'a', note('three'));
		await s.sync(SPACE);
		expect(await records()).toEqual([
			['a', 'three'],
			['b', 'two']
		]);
		expect(calls('getRepo')).toHaveLength(1);
		// One page per op, each with the commit and its MAC, and a fresh key share asked for on each.
		const shares = calls('listRepoOps').map((request) => request.headers.get('accept-signature'));
		expect(shares).toHaveLength(2);
		expect(new Set(shares).size).toBe(2);
		expect((await repoRow())?.lastError).toBeNull();
	});

	it('drops a record its oplog serves nested deeper than 32 levels, and syncs the rest', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);

		// The record is the first level, so 32 nested arrays in a field make 33.
		const extra = JSON.parse(`${'['.repeat(32)}0${']'.repeat(32)}`);
		await host.put(DJ, NOTE, 'a', { ...note('deep'), extra });
		await host.put(DJ, NOTE, 'b', note('two'));
		await s.sync(SPACE);

		expect(calls('listRepoOps')).toHaveLength(1);
		expect(await records()).toEqual([['b', 'two']]);
		expect(changes.findLast((change) => change.rkey === 'a')).toMatchObject({ kind: 'delete' });
		expect((await repoRow())?.lastError).toBeNull();
	});

	it.each([32, 4_000])(
		'downloads a repo holding a record nested in %i arrays, logging and leaving out that one',
		async (levels) => {
			const { logger, notes } = writtenLog();
			const s = await syncer(undefined, null, { logger });
			await host.put(DJ, NOTE, 'a', note('one'));
			// Written as DRISL by hand, since the encoder overflows its stack on a record this deep: { deep: [[...0...]] }.
			const deep = Uint8Array.of(0xa1, 0x64, ...Buffer.from('deep'), ...Array<number>(levels).fill(0x81), 0x00);
			await host.putEncoded(DJ, NOTE, 'b', deep);
			await host.put(DJ, NOTE, 'c', note('three'));

			await s.sync(SPACE);
			expect(await records()).toEqual([
				['a', 'one'],
				['c', 'three']
			]);
			expect((await repoRow())?.lastError).toBeNull();
			const warnings = notes().filter(({ level }) => level === 'warn');
			expect(warnings.map(({ message, uri }) => [message, uri])).toEqual([
				['dropping a synced record nested deeper than 32 levels', `${SPACE}/${DJ}/${NOTE}/b`]
			]);

			// The record still counts toward the repo's commit, so the oplog picks up from there.
			await host.put(DJ, NOTE, 'd', note('four'));
			await s.sync(SPACE);
			expect((await records()).at(-1)).toEqual(['d', 'four']);
			expect(calls('getRepo')).toHaveLength(1);
		}
	);

	describe('with one failing repo among good ones', () => {
		const BAD = 'did:plc:bad';
		const NOON = new Date('2026-11-04T12:00:00.000Z').getTime();

		const watch = () => db.selectFrom('spaceWatch').selectAll().executeTakeFirstOrThrow();
		const retries = () => db.selectFrom('syncRepoRetry').selectAll().execute();

		/** Two repos on a page each, the failing one first, synced once at noon. */
		const failOnce = async () => {
			vi.useFakeTimers({ toFake: ['Date'], now: NOON });
			onTestFinished(() => void vi.useRealTimers());
			const s = await syncer();
			host.repoPageSize = 1;
			await host.put(BAD, NOTE, 'a', note('late'));
			await host.put(DJ, NOTE, 'b', note('good'));
			host.repoErrors.set(BAD, 'UpstreamFailure');
			await expect(s.sync(SPACE)).rejects.toThrow('UpstreamFailure');

			return s;
		};

		it('syncs the later pages, and moves the cursor past it', async () => {
			await failOnce();

			expect(await records()).toEqual([['b', 'good']]);
			expect(await watch()).toMatchObject({ spaceRev: expect.any(String), lastError: 'UpstreamFailure' });
			expect(await retries()).toEqual([
				{
					spaceUri: SPACE,
					repoDid: BAD,
					attempts: 1,
					nextAttemptAt: '2026-11-04T12:01:00.000Z',
					lastError: 'UpstreamFailure'
				}
			]);
		});

		it('reports it with the space, though it never synced', async () => {
			const s = await failOnce();

			expect((await s.status())[0].repos).toEqual([
				{ did: DJ, rev: expect.any(String), lastSyncedAt: expect.any(String), lastError: null },
				{ did: BAD, rev: null, lastSyncedAt: null, lastError: 'UpstreamFailure' }
			]);
		});

		it('retries it after its backoff, though listWriters no longer lists it', async () => {
			const s = await failOnce();
			host.repoErrors.clear();

			vi.setSystemTime(NOON + 59_000);
			await s.sync(SPACE);
			expect(await records()).toEqual([['b', 'good']]);
			expect((await watch()).lastError).toBe('UpstreamFailure');

			vi.setSystemTime(NOON + 60_000);
			await s.sync(SPACE);
			expect(await records()).toEqual([
				['a', 'late'],
				['b', 'good']
			]);
			expect(await retries()).toEqual([]);
			expect((await watch()).lastError).toBeNull();
		});

		it('keeps to the backoff when the repo changes again', async () => {
			const s = await failOnce();
			const downloads = calls('getRepo').length;
			host.repoErrors.clear();
			await host.put(BAD, NOTE, 'c', note('later'));

			await s.sync(SPACE);

			expect(calls('getRepo')).toHaveLength(downloads);
			expect(await records()).toEqual([['b', 'good']]);
		});

		it('waits twice as long each time it fails again, up to an hour', async () => {
			const s = await failOnce();
			const minutes: number[] = [];

			for (let attempt = 0; attempt < 8; attempt++) {
				const [retry] = await retries();
				const at = Date.parse(retry.nextAttemptAt);
				minutes.push((at - Date.now()) / 60_000);
				vi.setSystemTime(at);
				await expect(s.sync(SPACE)).rejects.toThrow('UpstreamFailure');
			}

			expect(minutes).toEqual([1, 2, 4, 8, 16, 32, 60, 60]);
		});

		it('forgets it when the space is deleted', async () => {
			const s = await failOnce();
			host.credentialError = 'SpaceDeleted';

			await s.reauthorize(SPACE);

			expect(await retries()).toEqual([]);
		});
	});

	it('stops at a credential it cannot get, without blaming the repo or moving the cursor', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		const before = await db.selectFrom('spaceWatch').select('spaceRev').executeTakeFirstOrThrow();

		// The cached credential lists repos, then the repo host rejects it and a new one can't be had.
		await host.put(DJ, NOTE, 'b', note('two'));
		host.repoError = 'CredentialRevoked';
		host.credentialError = 'InvalidDelegationToken';

		await expect(s.sync(SPACE)).rejects.toThrow('InvalidDelegationToken');
		expect(await db.selectFrom('syncRepoRetry').selectAll().execute()).toEqual([]);
		expect(await db.selectFrom('spaceWatch').select('spaceRev').executeTakeFirstOrThrow()).toEqual(before);
	});

	it('refreshes a KEM key that rotated, and tries again', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		await host.rotateKemKey(DJ);
		await host.put(DJ, NOTE, 'b', note('two'));
		await s.sync(SPACE);
		expect(host.identity.refreshed).toEqual([DJ]);
		expect(await records()).toEqual([
			['a', 'one'],
			['b', 'two']
		]);
	});

	it('keeps nothing a MAC does not vouch for', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		host.forgeMacs = true;
		await host.put(DJ, NOTE, 'b', note('two'));
		await expect(s.sync(SPACE)).rejects.toThrow('MAC does not match');
		expect(await records()).toEqual([['a', 'one']]);
		expect((await repoRow())?.lastError).toBe('MAC does not match');
		// Once more after refreshing the KEM key, then it gives up.
		expect(host.identity.refreshed).toEqual([DJ]);
	});

	it("says so when an account's DID document has no KEM key", async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		host.identity.withoutKemKeys.add(DJ);
		await host.put(DJ, NOTE, 'b', note('two'));
		await expect(s.sync(SPACE)).rejects.toThrow('publishes no #atproto_kem key');
		expect((await repoRow())?.lastError).toMatch(/^did:plc:dj publishes no #atproto_kem key/);
		expect(await records()).toEqual([['a', 'one']]);
	});

	it('falls back to a full download that drops records the repo no longer has', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await host.put(DJ, NOTE, 'b', note('two'));
		await s.sync(SPACE);
		await host.delete(DJ, NOTE, 'a');
		// A local set hash that has drifted from the records, so the oplog can't be trusted to bring it up to date.
		await db
			.updateTable('syncedRepo')
			.set({ ltHash: RepoCommit.fromState(null).setHash.state() })
			.execute();
		await s.sync(SPACE);
		expect(calls('getRepo')).toHaveLength(2);
		expect(await records()).toEqual([['b', 'two']]);
		expect(changes.at(-1)).toMatchObject({ kind: 'delete', rkey: 'a' });
	});

	it('drops a space only once its authority refuses a fresh credential, and starts over when the space is back', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);

		// A notice that the space is gone, while its authority still has it, changes nothing.
		await s.reauthorize(SPACE);
		expect(host.issued).toHaveLength(2);
		expect(await records()).toEqual([['a', 'one']]);

		host.credentialError = 'SpaceDeleted';
		await s.reauthorize(SPACE);
		await s.reauthorize(SPACE);
		expect(changes.filter((change) => change.kind === 'delete')).toHaveLength(1);
		expect(dropped).toEqual([SPACE]);
		expect(await records()).toEqual([]);
		expect(await db.selectFrom('syncedRepo').selectAll().execute()).toEqual([]);
		expect(await db.selectFrom('spaceWatch').selectAll().execute()).toEqual([]);

		host.credentialError = null;
		await s.watch(SPACE);
		await s.sync(SPACE);
		expect(calls('getRepo')).toHaveLength(2);
		expect(await records()).toEqual([['a', 'one']]);
	});

	it('reads past a repo taken out of the writer set, keeping what it synced from it', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		const downloads = calls('getRepo').length;

		host.removeWriter(DJ);
		await s.sync(SPACE);

		expect(await records()).toEqual([['a', 'one']]);
		expect(calls('getRepo')).toHaveLength(downloads);
		expect(calls('listRepoOps')).toHaveLength(0);
		expect((await db.selectFrom('spaceWatch').select('spaceRev').executeTakeFirst())?.spaceRev).toBe(
			host.removed.get(DJ)
		);
	});

	describe('with a notify endpoint', () => {
		const ENDPOINT = 'https://localnetwork.test';
		const watchRow = () => db.selectFrom('spaceWatch').selectAll().executeTakeFirstOrThrow();

		it("registers a watched space for its updates with the space's own token, until the credential runs out", async () => {
			const s = await syncer(undefined, ENDPOINT);
			await s.sync(SPACE);
			const { notifyToken, registrationExpiresAt } = await watchRow();
			const [registration] = host.registrations;

			expect(host.registrations).toEqual([{ endpoint: ENDPOINT, token: notifyToken, expiresAt: expect.any(Number) }]);
			expect(registrationExpiresAt).toBe(new Date(registration.expiresAt).toISOString());
		});

		it('knows the token it registered a space with, and no other', async () => {
			const s = await syncer(undefined, ENDPOINT);
			const { notifyToken } = await watchRow();

			expect(await s.isRegistered(SPACE, notifyToken)).toBe(true);
			expect(await s.isRegistered(SPACE, `${notifyToken}x`)).toBe(false);
			expect(await s.isRegistered(SPACE, notifyToken.slice(1))).toBe(false);
			expect(await s.isRegistered(bookingSpace('did:plc:root', 'other'), notifyToken)).toBe(false);
		});

		it('keeps syncing, unregistered, when the host cannot confirm the endpoint', async () => {
			const s = await syncer(undefined, ENDPOINT);
			host.confirmsRegistration = false;
			await host.put(DJ, NOTE, 'a', note('one'));

			await s.sync(SPACE);

			expect(await records()).toEqual([['a', 'one']]);
			expect(await watchRow()).toMatchObject({ registrationExpiresAt: null, lastError: null });
		});

		it('asks nothing of a space it does not watch', async () => {
			const s = await syncer(undefined, ENDPOINT);

			expect(await s.registration(bookingSpace('did:plc:root', 'other'))).toBeNull();
		});
	});

	it("keeps a space a writer's repo host says was deleted, since only the authority can say so", async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		host.repoError = 'SpaceDeleted';
		await host.put(DJ, NOTE, 'b', note('two'));
		await expect(s.sync(SPACE)).rejects.toThrow('SpaceDeleted');
		expect(dropped).toEqual([]);
		expect(await records()).toEqual([['a', 'one']]);
		expect((await repoRow())?.lastError).toBe('SpaceDeleted');
		expect(await db.selectFrom('spaceWatch').select('lastError').execute()).toEqual([{ lastError: 'SpaceDeleted' }]);
	});

	it('drops a space its authority says was deleted, and keeps one whose delegation token it refused', async () => {
		const s = await syncer();
		host.credentialError = 'InvalidDelegationToken';
		await expect(s.sync(SPACE)).rejects.toThrow('InvalidDelegationToken');
		expect(await db.selectFrom('spaceWatch').select('lastError').execute()).toEqual([
			{ lastError: 'InvalidDelegationToken' }
		]);

		host.credentialError = 'SpaceDeleted';
		await s.sync(SPACE);
		expect(await db.selectFrom('spaceWatch').selectAll().execute()).toEqual([]);
	});

	/** Has the host answer the methods in `nsids` with `edit(response)` instead, as a broken or hostile host could. */
	const tamper = (nsids: string[], edit: (response: Response, url: URL) => Promise<Response>) => {
		const real = host.fetch;
		vi.spyOn(host, 'fetch').mockImplementation(async (input, init) => {
			const url = new URL(input instanceof Request ? input.url : input);
			const nsid = url.pathname.replace('/xrpc/com.atproto.space.', '');
			const response = await real(input, init);

			return nsids.includes(nsid) ? edit(response, url) : response;
		});
	};

	/** Rewrites a listRepoOps page's JSON, keeping its MAC header. */
	const editOps = (edit: (page: { ops: { value?: object }[]; commit?: object }) => void) =>
		tamper(['listRepoOps'], async (response) => {
			const page = await response.json();
			edit(page);

			return new Response(JSON.stringify(page), { headers: response.headers });
		});

	describe('a listRepoOps page from a broken or hostile host', () => {
		/** What the oplog's first page is swapped for: any text, other values for its ops, one field changed, or cut off. */
		type PageEdit =
			| { kind: 'text'; text: string }
			| { kind: 'values'; values: JsonValue[] }
			| { kind: 'field'; field: (typeof FIELDS)[number]; value: JsonValue | undefined }
			| { kind: 'cut'; at: number };

		const pageEdit = (collection: string): fc.Arbitrary<PageEdit> =>
			fc.oneof(
				fc.record({ kind: fc.constant('text' as const), text: fc.oneof(fc.string(), fc.json()) }),
				fc.record({ kind: fc.constant('values' as const), values: fc.array(opValue(collection), { minLength: 1 }) }),
				fc.record({
					kind: fc.constant('field' as const),
					field: fc.constantFrom(...FIELDS),
					value: fc.option(fc.jsonValue(), { nil: undefined })
				}),
				fc.record({ kind: fc.constant('cut' as const), at: fc.nat() })
			);

		function serve(edit: PageEdit, text: string): string {
			switch (edit.kind) {
				case 'text':
					return edit.text;
				case 'cut':
					return text.slice(0, edit.at % text.length);
				case 'field':
					return write(withFieldSet(JSON.parse(text), edit.field, edit.value));
				case 'values': {
					const page: { ops: { value?: JsonValue }[] } = JSON.parse(text);
					page.ops.forEach((op, index) => {
						if (index < edit.values.length) op.value = edit.values[index];
					});

					return write(page);
				}
			}
		}

		const stored = async () =>
			Object.fromEntries(
				(await db.selectFrom('spaceRecord').select(['rkey', 'value']).execute()).map(({ rkey, value }) => [
					rkey,
					JSON.parse(value)
				])
			);

		it('stores the ops of a page it reads, falls back to a full download otherwise, and keeps nothing too deep to validate', async () => {
			await fc.assert(
				fc.asyncProperty(
					fc
						.constantFrom(...RECORD_COLLECTIONS)
						.chain((collection) => fc.tuple(fc.constant(collection), pageEdit(collection))),
					async ([collection, edit]) => {
						await db.destroy();
						db = await openDatabase(':memory:');
						host = await FakeSpaceHost.create(SPACE);
						changes = [];
						const record = (text: string) => ({ $type: collection, text });

						const s = await syncer();
						await host.put(DJ, collection, 'a', record('one'));
						await s.sync(SPACE);
						await host.put(DJ, collection, 'b', record('two'));
						await host.put(DJ, collection, 'a', record('three'));

						// The pages of the last read of the oplog. Only its first page is swapped, every time it's read, so a cursor
						// the swap makes up reaches the real host and ends there.
						let served: string[] = [];
						tamper(['listRepoOps'], async (response, url) => {
							const text = await response.text();

							if (url.searchParams.has('cursor')) served.push(text);
							else served = [serve(edit, text)];

							return new Response(served.at(-1), { headers: response.headers });
						});

						const failed = await s.syncRepo(SPACE, DJ).then(
							() => null,
							(error: Error) => error
						);

						// Whatever went in, nothing comes out too deep for validateRecord to read.
						const deep = changes.filter(
							(change) => change.kind === 'upsert' && isTooDeepFor(change.collection, change.value)
						);

						expect(deep).toEqual([]);

						// Only a MAC that fails twice fails the repo, which keeps what it had and waits for its retry. Any other page
						// it can't use, it downloads in full instead. A page it reads is the local copy's ops, with a value too deep
						// to validate dropped.
						const read = () => {
							const copy = new Map<string, unknown>([['a', record('one')]]);

							for (const page of served) {
								const { ops }: { ops: { collection: string; rkey: string; cid: string | null; value?: JsonObject }[] } =
									JSON.parse(page);

								for (const op of ops) {
									if (op.cid === null || (op.value && isTooDeepFor(op.collection, op.value))) copy.delete(op.rkey);
									else copy.set(op.rkey, op.value && lexToJson(jsonToLex(op.value)));
								}
							}

							return Object.fromEntries(copy);
						};

						const downloaded = calls('getRepo').length > 1;
						const kept = failed ? { a: record('one') } : downloaded && { a: record('three'), b: record('two') };
						expect(await stored()).toEqual(kept || read());
						expect((await repoRow())?.lastError).toBe(failed?.message ?? null);
					}
				),
				{ numRuns: 50 }
			);
		}, 60_000);
	});

	it('reports each watched space with its repos, and skips repos already up to date when listing from the start', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		const synced = await repoRow();

		expect(await s.status()).toEqual([
			{
				space: SPACE,
				type: SPACE_TYPE.booking,
				skey: 'gig',
				spaceRev: synced?.rev,
				lastSyncedAt: expect.any(String),
				lastError: null,
				registrationExpiresAt: null,
				repos: [{ did: DJ, rev: synced?.rev, lastSyncedAt: synced?.lastSyncedAt, lastError: null }],
				stuck: []
			}
		]);

		await db.updateTable('spaceWatch').set({ spaceRev: null }).execute();
		await s.sync(SPACE);
		expect(calls('getRepo')).toHaveLength(1);
		expect(calls('listRepoOps')).toHaveLength(0);
		expect(await repoRow()).toEqual(synced);
	});

	it('takes only the latest value of a record written twice since the last pass', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		await host.put(DJ, NOTE, 'a', note('two'));
		await host.put(DJ, NOTE, 'a', note('three'));
		changes = [];

		await s.sync(SPACE);
		expect(await records()).toEqual([['a', 'three']]);
		expect(changes.map((change) => change.kind === 'upsert' && change.value.text)).toEqual(['three']);
		expect(calls('getRepo')).toHaveLength(1);
	});

	it('downloads the repo in full when an oplog page comes without its commit', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		editOps((page) => delete page.commit);
		await host.put(DJ, NOTE, 'b', note('two'));

		await s.sync(SPACE);
		expect(calls('getRepo')).toHaveLength(2);
		expect(await records()).toEqual([
			['a', 'one'],
			['b', 'two']
		]);
	});

	it('downloads the repo in full when the oplog leaves out a current value', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		editOps((page) => page.ops.forEach((op) => delete op.value));
		await host.put(DJ, NOTE, 'b', note('two'));

		await s.sync(SPACE);
		expect(calls('getRepo')).toHaveLength(2);
		expect(await records()).toEqual([
			['a', 'one'],
			['b', 'two']
		]);
	});

	describe('a host whose listing never ends', () => {
		/** Gives each `nsid` page a new cursor, keeping its MAC, then fails the 1,000th time so a test can't hang. */
		const endless = (nsid: 'listWriters' | 'listRepoOps') => {
			let answers = 0;
			tamper([nsid], async (response) => {
				if (++answers >= 1000) return Response.json({ error: 'TooManyPages' }, { status: 400 });
				const page = await response.json();
				// The writer listing would come back empty past the repo's own revision, which ends it.
				const repos = [{ $type: 'com.atproto.space.defs#repoWrite', repo: DJ, repoRev: '0', spaceRev: '0' }];

				return new Response(
					JSON.stringify({ ...page, ...(nsid === 'listWriters' && { repos }), cursor: String(answers) }),
					{ headers: response.headers }
				);
			});
		};

		// A cap of 3 keeps these to a few round trips to the host; 'reads 100 pages of a repo list a pass' pins the default.
		it('downloads the repo in full once its oplog runs past the page cap', async () => {
			const s = await syncer(undefined, null, { maxPages: 3 });
			await host.put(DJ, NOTE, 'a', note('one'));
			await s.sync(SPACE);
			await host.put(DJ, NOTE, 'b', note('two'));
			endless('listRepoOps');

			await s.sync(SPACE);
			expect(calls('listRepoOps')).toHaveLength(3);
			expect(calls('getRepo')).toHaveLength(2);
			expect(await records()).toEqual([
				['a', 'one'],
				['b', 'two']
			]);
			expect((await repoRow())?.lastError).toBeNull();
		});

		it('reads its repo list up to the page cap a pass, keeping its place for the next', async () => {
			const s = await syncer(undefined, null, { maxPages: 3 });
			await host.put(DJ, NOTE, 'a', note('one'));
			await s.sync(SPACE);
			endless('listWriters');

			await s.sync(SPACE);
			expect(calls('listWriters')).toHaveLength(4);
			expect(await db.selectFrom('spaceWatch').selectAll().executeTakeFirst()).toMatchObject({
				spaceRev: '3',
				lastError: null
			});
		});
	});

	it('leaves a repo alone when its oplog has nothing new', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		const synced = await repoRow();
		changes = [];

		await s.syncRepo(SPACE, DJ);
		expect(changes).toEqual([]);
		expect(calls('listRepoOps')).toHaveLength(1);
		expect(calls('getRepo')).toHaveLength(1);
		expect(await repoRow()).toEqual({ ...synced, lastSyncedAt: expect.any(String) });
	});

	it('fails a first download that comes back empty, with no repo row to put the error on', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		tamper(['getRepo'], async (response) => new Response(null, { headers: response.headers }));

		await expect(s.sync(SPACE)).rejects.toThrow('Empty repo response');
		expect(await repoRow()).toBeUndefined();
		expect((await db.selectFrom('spaceWatch').select('lastError').executeTakeFirst())?.lastError).toBe(
			'Empty repo response'
		);
	});

	/** Has the host's getRepo archive rewritten by `edit`, keeping its MAC header. */
	const editArchive = (edit: (archive: Buffer) => Buffer) =>
		tamper(['getRepo'], async (response) => {
			const archive = edit(Buffer.from(await response.arrayBuffer()));

			return new Response(new Uint8Array(archive), { headers: response.headers });
		});

	/** `archive` with the first `text` in it replaced, byte for byte. */
	const replaced = (archive: Buffer, text: string, by: string) =>
		Buffer.from(archive.toString('latin1').replace(text, by), 'latin1');

	it('reads a repo archive framed with control bytes, as the alpha host has sent since Nov 5', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		const served: Buffer[] = [];
		editArchive((archive) => (served.push(archive), archive));

		await s.sync(SPACE);
		expect(await records()).toEqual([['a', 'one']]);
		// Each record follows a 0x01 byte, and a 0x00 byte ends the archive.
		const path = Buffer.from(`${NOTE}/a`);
		expect(served[0].includes(Buffer.concat([Buffer.from([1, path.length]), path]))).toBe(true);
		expect(served[0].at(-1)).toBe(0);
	});

	describe('a repo archive that does not read', () => {
		const PATH = `${NOTE}/a`;

		it.each([
			['is cut off before its end byte', (archive: Buffer) => archive.subarray(0, -1), 'unexpected end of archive'],
			[
				'has a record path that is not one',
				(archive: Buffer) => archive.fill('&', archive.indexOf(PATH), archive.indexOf(PATH) + 1),
				`invalid NSID in record path: &${PATH.slice(1)}`
			],
			[
				'has a record that does not fold into its commit',
				(archive: Buffer) => replaced(archive, 'text\x63one', 'text\x63onf'),
				'records do not match the commit data'
			],
			[
				'is another kind of archive',
				(archive: Buffer) => replaced(archive, 'atproto-space-repo', 'atproto-space-rapo'),
				'invalid atproto-space-repo metadata'
			]
		])("fails the repo when it %s, with the reader's error, and keeps the KEM key", async (_, edit, message) => {
			const s = await syncer();
			await host.put(DJ, NOTE, 'a', note('one'));
			editArchive(edit);

			await expect(s.sync(SPACE)).rejects.toThrow(message);
			expect(host.identity.refreshed).toEqual([]);
			expect(calls('getRepo')).toHaveLength(1);
			expect(await db.selectFrom('syncRepoRetry').select('lastError').execute()).toEqual([{ lastError: message }]);
		});

		it('fails the repo when it lists its records out of order', async () => {
			const s = await syncer();
			await host.put(DJ, NOTE, 'a', note('one'));
			await host.put(DJ, NOTE, 'b', note('two'));
			// Each record's path swapped for the other's, by way of a third, so b's comes first.
			const [OTHER, SWAP] = [`${NOTE}/b`, `${NOTE}/x`];
			editArchive((archive) => replaced(replaced(replaced(archive, PATH, SWAP), OTHER, PATH), SWAP, OTHER));

			await expect(s.sync(SPACE)).rejects.toThrow(`records out of order at ${PATH}`);
			expect(await records()).toEqual([]);
		});

		it.each([
			['is not a map', encode(['one']), `invalid record at ${PATH}`],
			['is not DRISL', Uint8Array.of(0xa1, 0x64), `invalid record cbor at ${PATH}`]
		])('fails the repo when a record %s', async (_, bytes, message) => {
			const s = await syncer();
			await host.putEncoded(DJ, NOTE, 'a', bytes);

			await expect(s.sync(SPACE)).rejects.toThrow(message);
			expect(await records()).toEqual([]);
		});
	});

	describe('a repo export that runs past the size cap', () => {
		const MAX_BYTES = 64 * 1024;

		/** Has the host's getRepo archive go on past its last record with records forever, counting the bytes sent. */
		const endless = () => {
			let sent = 0;
			tamper(['getRepo'], async (response) => {
				const archive = new Uint8Array(await response.arrayBuffer());
				const value = encode(note('more'));
				let count = 0;

				const body = new ReadableStream<Uint8Array>({
					pull(controller) {
						// The archive without its end byte, then one record after another, in path order. Each length fits
						// a one-byte varint.
						const key = Buffer.from(`${NOTE}/z${String(count).padStart(12, '0')}`);

						const chunk =
							count++ === 0 ? archive.subarray(0, -1) : Uint8Array.of(1, key.length, ...key, value.length, ...value);

						sent += chunk.length;
						controller.enqueue(chunk);
					}
				});

				return new Response(body, { headers: response.headers });
			});

			return () => sent;
		};

		it('stops reading it, logs the repo, and keeps the local copy and its place until a retry works', async () => {
			const NOON = new Date('2026-11-04T12:00:00.000Z').getTime();
			vi.useFakeTimers({ toFake: ['Date'], now: NOON });
			onTestFinished(() => void vi.useRealTimers());
			const { logger, notes } = writtenLog();
			const s = await syncer(undefined, null, { logger, maxRepoBytes: MAX_BYTES });
			await host.put(DJ, NOTE, 'a', note('one'));
			await s.sync(SPACE);
			// A local set hash that has drifted from the records, so the next pass downloads the repo in full.
			const drifted = RepoCommit.fromState(null).setHash.state();
			await db.updateTable('syncedRepo').set({ ltHash: drifted }).execute();
			const synced = await repoRow();
			await host.put(DJ, NOTE, 'b', note('two'));
			const sent = endless();

			await expect(s.sync(SPACE)).rejects.toThrow('Response too large');
			expect(sent()).toBeLessThan(2 * MAX_BYTES);
			expect(notes()).toContainEqual(
				expect.objectContaining({
					level: 'warn',
					message: 'repo export ran past the size cap; giving up on it',
					repoDid: DJ,
					maxBytes: MAX_BYTES
				})
			);
			expect(await records()).toEqual([['a', 'one']]);
			expect(await repoRow()).toEqual({ ...synced, lastError: 'Response too large' });
			expect(await db.selectFrom('syncRepoRetry').select(['repoDid', 'attempts']).execute()).toEqual([
				{ repoDid: DJ, attempts: 1 }
			]);

			vi.mocked(host.fetch).mockRestore();
			vi.setSystemTime(NOON + 60_000);
			await s.sync(SPACE);
			expect(await records()).toEqual([
				['a', 'one'],
				['b', 'two']
			]);
			expect(await db.selectFrom('syncRepoRetry').selectAll().execute()).toEqual([]);
			expect((await db.selectFrom('spaceWatch').select('lastError').executeTakeFirstOrThrow()).lastError).toBeNull();
		});
	});

	it('refreshes the KEM key once when a full download fails its MAC', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		host.forgeMacs = true;

		await expect(s.sync(SPACE)).rejects.toThrow('MAC does not match');
		expect(host.identity.refreshed).toEqual([DJ]);
		expect(calls('getRepo')).toHaveLength(2);
		expect(await records()).toEqual([]);
	});

	it.each(['RepoTakendown', 'RepoSuspended', 'RepoDeactivated'])(
		'skips a writer whose repo host answers %s, without failing the pass or downloading the repo',
		async (error) => {
			const WRITER = 'did:plc:writer';
			const s = await syncer();
			await host.put(DJ, NOTE, 'a', note('one'));
			await host.put(WRITER, NOTE, 'b', note('two'));
			await s.sync(SPACE);
			const downloads = calls('getRepo').length;

			host.repoErrors.set(WRITER, error);
			await host.put(WRITER, NOTE, 'c', note('three'));
			await host.put(DJ, NOTE, 'd', note('four'));
			await s.sync(SPACE);

			expect(calls('getRepo')).toHaveLength(downloads);
			expect(await records()).toEqual([
				['a', 'one'],
				['b', 'two'],
				['d', 'four']
			]);
			expect(await db.selectFrom('syncRepoRetry').select(['repoDid', 'lastError']).execute()).toEqual([
				{ repoDid: WRITER, lastError: error }
			]);
			const status = (await s.status())[0];
			expect(status.lastError).toBe(error);
			expect(status.repos.find((repo) => repo.did === WRITER)?.lastError).toBe(error);
		}
	);

	it.each(['RepoTakendown', 'RepoDeactivated'])(
		'keeps a space whose authority answers %s, since that only suspends it',
		async (error) => {
			await host.put(DJ, NOTE, 'a', note('one'));
			await (await syncer()).sync(SPACE);

			host.overrides.set('com.atproto.space.listWriters', () => Response.json({ error }, { status: 400 }));
			await expect((await syncer()).sync(SPACE)).rejects.toThrow(error);
			host.overrides.clear();
			host.credentialError = error;
			// A new syncer has no credential cached, so it asks the authority for one.
			await expect((await syncer()).sync(SPACE)).rejects.toThrow(error);

			expect(dropped).toEqual([]);
			expect(await records()).toEqual([['a', 'one']]);
			expect(await db.selectFrom('syncRepoRetry').selectAll().execute()).toEqual([]);
			expect(await db.selectFrom('spaceWatch').select('lastError').execute()).toEqual([{ lastError: error }]);
		}
	);

	it('keeps a failure that is not an Error as its text on the repo', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		await host.put(DJ, NOTE, 'b', note('two'));
		tamper(['listRepoOps', 'getRepo'], () => Promise.reject('connection reset'));

		await expect(s.sync(SPACE)).rejects.toBe('connection reset');
		expect((await repoRow())?.lastError).toBe('connection reset');
	});

	it('stores records it could not project, and drops a space whose deletes it could not project', async () => {
		const s = await syncer(async () => Promise.reject(new Error('projection broke')));
		await host.put(DJ, NOTE, 'a', note('one'));

		await s.sync(SPACE);
		expect(await records()).toEqual([['a', 'one']]);
		host.credentialError = 'SpaceDeleted';
		await s.reauthorize(SPACE);
		expect(await records()).toEqual([]);
		expect(dropped).toEqual([SPACE]);
	});

	describe('a record whose projection fails', () => {
		const NOON = new Date('2026-11-04T12:00:00.000Z');
		const pending = () => db.selectFrom('pendingProjection').selectAll().execute();

		/** Syncs with a projection that fails while `broken` is set, stepping the clock with the returned `after`. */
		const failing = async () => {
			vi.useFakeTimers({ toFake: ['Date'], now: NOON });
			onTestFinished(() => void vi.useRealTimers());
			const seen: SyncedChange[] = [];
			const state = { broken: true, seen };

			const s = await syncer(async (change) => {
				if (state.broken) throw new Error('directory offline');
				state.seen.push(change);
			});

			return { s, state };
		};

		const after = (ms: number) => vi.setSystemTime(NOON.getTime() + ms);

		it('is projected by a later pass, though its repo has nothing new', async () => {
			const { s, state } = await failing();
			await host.put(DJ, NOTE, 'a', note('one'));
			await s.sync(SPACE);
			expect((await repoRow())?.rev).toBeTruthy();
			expect(await pending()).toMatchObject([{ rkey: 'a', attempts: 1, lastError: 'directory offline' }]);
			expect((await s.status())[0]?.stuck).toEqual([
				{
					did: DJ,
					collection: NOTE,
					rkey: 'a',
					attempts: 1,
					nextAttemptAt: '2026-11-04T12:01:00.000Z',
					lastError: 'directory offline'
				}
			]);

			state.broken = false;
			after(60_000);
			await s.sync(SPACE);
			expect(state.seen).toMatchObject([{ kind: 'upsert', rkey: 'a', value: { text: 'one' } }]);
			expect(await pending()).toEqual([]);
			expect((await s.status())[0]?.stuck).toEqual([]);
		});

		it('shows a failure that is not an Error as its text', async () => {
			const s = await syncer(() => Promise.reject('connection reset'));
			await host.put(DJ, NOTE, 'a', note('one'));
			await s.sync(SPACE);
			expect((await s.status())[0]?.lastError).toBe('connection reset');
		});

		it('waits out a backoff that doubles each time, and shows on the space until it clears', async () => {
			const { s, state } = await failing();
			await host.put(DJ, NOTE, 'a', note('one'));
			await s.sync(SPACE);
			expect((await s.status())[0]?.lastError).toBe('directory offline');

			state.broken = false;
			await s.sync(SPACE);
			expect(state.seen).toEqual([]);

			state.broken = true;
			after(60_000);
			await s.sync(SPACE);
			expect(await pending()).toMatchObject([{ attempts: 2, nextAttemptAt: '2026-11-04T12:03:00.000Z' }]);

			state.broken = false;
			after(180_000);
			await s.sync(SPACE);
			expect(state.seen).toHaveLength(1);
			expect((await s.status())[0]?.lastError).toBeNull();
		});

		it('projects the value the record has by then', async () => {
			const { s, state } = await failing();
			await host.put(DJ, NOTE, 'a', note('one'));
			await s.sync(SPACE);

			state.broken = false;
			await host.put(DJ, NOTE, 'a', note('two'));
			await s.sync(SPACE);
			expect(state.seen).toMatchObject([{ kind: 'upsert', value: { text: 'two' } }]);
			expect(await pending()).toEqual([]);
		});

		it('projects a delete that failed as a delete', async () => {
			const { s, state } = await failing();
			state.broken = false;
			await host.put(DJ, NOTE, 'a', note('one'));
			await s.sync(SPACE);
			state.seen.length = 0;

			state.broken = true;
			await host.delete(DJ, NOTE, 'a');
			await s.sync(SPACE);
			expect(await records()).toEqual([]);

			state.broken = false;
			after(60_000);
			await s.sync(SPACE);
			expect(state.seen).toMatchObject([{ kind: 'delete', rkey: 'a' }]);
		});

		it('runs one that never ran because the server stopped after storing it', async () => {
			const { s, state } = await failing();
			state.broken = false;
			await host.put(DJ, NOTE, 'a', note('one'));
			await s.sync(SPACE);
			const { cid, value } = await db.selectFrom('spaceRecord').selectAll().executeTakeFirstOrThrow();

			await db
				.insertInto('pendingProjection')
				.values({
					spaceUri: SPACE,
					repoDid: DJ,
					collection: NOTE,
					rkey: 'a',
					attempts: 0,
					nextAttemptAt: nowIso(),
					lastError: null
				})
				.execute();
			// Only waiting for its first run, so not stuck.
			expect((await s.status())[0]?.stuck).toEqual([]);
			state.seen.length = 0;
			await s.sync(SPACE);
			expect(state.seen).toMatchObject([{ kind: 'upsert', cid, value: JSON.parse(value) }]);
		});

		it('goes with its space, which still hears about a delete that never got projected', async () => {
			const { s, state } = await failing();
			state.broken = false;
			await host.put(DJ, NOTE, 'a', note('one'));
			await s.sync(SPACE);
			state.seen.length = 0;
			state.broken = true;
			await host.delete(DJ, NOTE, 'a');
			await s.sync(SPACE);

			state.broken = false;
			host.credentialError = 'SpaceDeleted';
			await s.reauthorize(SPACE);
			expect(state.seen).toMatchObject([{ kind: 'delete', rkey: 'a' }]);
			expect(await pending()).toEqual([]);
		});
	});
});

describe('readOpsPage', () => {
	const NSID = 'com.atproto.space.listRepoOps';
	const CID = 'bafyreie5737gdxlw5i64vzichcalba3z2v5n6icifvx5xytvske7mr3hpm';
	const commit = { ver: 1, repo: 'did:plc:dj', space: SPACE, rev: '3m5qbpbz4ws2c', data: { $bytes: 'AAAA' } };

	const op = fc.constantFrom(...RECORD_COLLECTIONS).chain((collection) =>
		fc.record({
			collection: fc.constant(collection),
			rkey: recordKey(collection),
			cid: fc.constant(CID),
			prev: fc.constant(null),
			value: opValue(collection)
		})
	);

	const page = fc.record({
		ops: fc.array(op),
		commit: fc.constant(commit),
		cursor: fc.option(fc.string(), { nil: undefined })
	});

	const withField = fc
		.tuple(page, fc.constantFrom(...FIELDS), fc.option(fc.jsonValue(), { nil: undefined }))
		.map(([body, field, value]) => withFieldSet(body, field, value));

	it('reads any text as a page, or rejects it as the host failing listRepoOps', () => {
		const text = fc.oneof(
			fc.string(),
			fc.json(),
			page.map(write),
			withField.map(write),
			// Cut off anywhere.
			fc.tuple(page.map(write), fc.nat()).map(([whole, at]) => whole.slice(0, at % whole.length)),
			// Nested past the stack somewhere other than a record.
			fc
				.tuple(page, fc.integer({ min: 1_000, max: 30_000 }))
				.map(([body, levels]) => write({ ...body, extra: nested(levels) }))
		);

		/** What readOpsPage makes of `body`: how many ops it read and how many values among them are too deep, or how it failed. */
		function outcome(body: string) {
			try {
				const { ops } = readOpsPage(body).page;

				return {
					ops: ops.length,
					deep: ops.filter(({ collection, value }) => value && isTooDeepFor(collection, asJson(value))).length
				};
			} catch (error) {
				return {
					rejected:
						error instanceof XrpcResponseError ? { status: error.status, error: error.error, nsid: error.nsid } : error
				};
			}
		}

		fc.assert(
			fc.property(text, (body) => {
				const read = outcome(body);

				expect(read).toEqual(
					'rejected' in read
						? { rejected: { status: 502, error: 'InvalidResponse', nsid: NSID } }
						: { ops: JSON.parse(body).ops.length, deep: 0 }
				);
			})
		);
	});

	it('keeps every op of a page that reads, holding back only the values validateRecord refuses as too deep', () => {
		fc.assert(
			fc.property(page.map(write), (text) => {
				const sent: { collection: string; value: JsonObject }[] = JSON.parse(text).ops;

				const { page: read, tooDeep } = readOpsPage(text);

				// Each op as it went out, with its value held back exactly when it was too deep, and every value kept readable.
				expect(
					read.ops.map(({ collection, value }, index) => ({
						index,
						held: tooDeep.has(index),
						kept: value === undefined ? null : isTooDeepFor(collection, asJson(value))
					}))
				).toEqual(
					sent.map(({ collection, value }, index) => {
						const deep = isTooDeepFor(collection, value);

						return { index, held: deep, kept: deep ? null : false };
					})
				);
			})
		);
	});
});
