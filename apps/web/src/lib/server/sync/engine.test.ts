import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SpaceCredentialManager, XrpcResponseError } from '../atproto/credentials';
import { IdentityResolver } from '../atproto/identity';
import { bookingSpace } from '../atproto/spaces';
import { type Db, nowIso, openDatabase } from '../db';
import { silentLogger } from '../logger';
import { type SyncedChange, SpaceSyncer } from './engine';

const SPACE = bookingSpace('did:plc:root', 'gig');

describe('SpaceSyncer', () => {
	let db: Db;
	let passes: number;
	let changes: SyncedChange[];

	beforeEach(async () => {
		db = await openDatabase(':memory:');
		passes = 0;
		changes = [];
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
				spaceHost: 'https://pds.test',
				spaceKey: ''
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
	});
});
