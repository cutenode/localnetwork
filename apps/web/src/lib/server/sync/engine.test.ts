import { RepoCommit, WANT_MAC_HEADER } from '@atproto/space';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SpaceCredentialManager, XrpcResponseError } from '../atproto/credentials';
import { IdentityResolver } from '../atproto/identity';
import { bookingSpace } from '../atproto/spaces';
import { type Db, nowIso, openDatabase } from '../db';
import { silentLogger } from '../logger';
import { FakeSpaceHost } from '../testing/space-host';
import { type SyncedChange, SpaceSyncer } from './engine';

const SPACE = bookingSpace('did:plc:root', 'gig');

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

	// Every pass resolves the space authority first, so `passes` counts passes. `listReposError`
	// makes the space host's listRepos fail; otherwise it reports no repos.
	const syncer = (listReposError?: Error) => {
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

		const listRepos = vi.spyOn(credentials, 'query');

		if (listReposError) listRepos.mockRejectedValue(listReposError);
		else listRepos.mockResolvedValue({ repos: [] });

		return new SpaceSyncer({
			db,
			logger: silentLogger,
			notifyService: null,
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
		const s = syncer(new XrpcResponseError(400, 'SpaceNotFound'));

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

	const syncer = async () => {
		const credentials = new SpaceCredentialManager({
			source: async () => ({ getDelegationToken: async () => 'token' }),
			identity: host.identity,
			logger: silentLogger,
			fetch: host.fetch
		});

		const s = new SpaceSyncer({
			db,
			logger: silentLogger,
			notifyService: null,
			onChange: async (change) => void changes.push(change),
			onSpaceDropped: async (space) => void dropped.push(space),
			identity: host.identity,
			credentials
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
		// One page per op, and a fresh key share on each, though only the last page carries the commit.
		const shares = calls('listRepoOps').map((request) => request.headers.get(WANT_MAC_HEADER));
		expect(shares).toHaveLength(2);
		expect(new Set(shares).size).toBe(2);
		expect((await repoRow())?.lastError).toBeNull();
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
		await expect(s.sync(SPACE)).rejects.toThrow('Commit does not match its MAC');
		expect(await records()).toEqual([['a', 'one']]);
		expect((await repoRow())?.lastError).toBe('Commit does not match its MAC');
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

	it('purges a deleted space however often it hears, and starts over when the space is back', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		await s.sync(SPACE);
		await s.onSpaceDeleted(SPACE);
		await s.onSpaceDeleted(SPACE);
		expect(changes.filter((change) => change.kind === 'delete')).toHaveLength(1);
		expect(dropped).toEqual([SPACE, SPACE]);
		expect(await records()).toEqual([]);
		expect(await db.selectFrom('syncedRepo').selectAll().execute()).toEqual([]);
		expect(await db.selectFrom('spaceWatch').selectAll().execute()).toEqual([]);

		await s.watch(SPACE);
		await s.sync(SPACE);
		expect(calls('getRepo')).toHaveLength(2);
		expect(await records()).toEqual([['a', 'one']]);
	});

	it('waits for a running pass before purging, so it cannot write the space back', async () => {
		const s = await syncer();
		await host.put(DJ, NOTE, 'a', note('one'));
		const running = s.sync(SPACE);
		await s.onSpaceDeleted(SPACE);
		await running;
		expect(await records()).toEqual([]);
		expect(await db.selectFrom('syncedRepo').selectAll().execute()).toEqual([]);
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
});
