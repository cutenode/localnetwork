import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { SpaceCredentialManager } from '../atproto/credentials';
import { silentLogger } from '../logger';
import { createTestServices, ROOT } from '../testing/core';
import { FakeSpaceHost } from '../testing/space-host';
import { SpaceSyncer } from './engine';
import { createProjections } from './projections';

const ALICE = 'did:plc:alice';

describe('syncing the moderation space into an empty database', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let host: FakeSpaceHost;

	beforeEach(async () => {
		t = await createTestServices();
		host = await FakeSpaceHost.create(t.spaces.moderation);
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const sync = async () => {
		const syncer = new SpaceSyncer({
			db: t.db,
			logger: silentLogger,
			notifyService: null,
			onChange: createProjections(t),
			onSpaceDropped: async () => undefined,
			identity: host.identity,
			credentials: new SpaceCredentialManager({
				source: async () => ({ getDelegationToken: async () => 'token' }),
				identity: host.identity,
				logger: silentLogger
			})
		});

		await syncer.watch(t.spaces.moderation);
		await syncer.sync(t.spaces.moderation);
	};

	// A full download lists a repo's records by path, so the root's decisions come before the requests it filed.
	it('keeps the decision on a request the root filed for someone, which syncs after it', async () => {
		const request = `${t.spaces.moderation}/${ROOT}/${COLLECTION.request}/3req`;
		const createdAt = '2026-01-01T00:00:00.000Z';

		await host.put(ROOT, COLLECTION.request, '3req', {
			$type: COLLECTION.request,
			kind: 'create',
			profile: { name: 'Filed', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
			onBehalfOf: ALICE,
			createdAt
		});
		await host.put(ROOT, COLLECTION.decision, '3dec', {
			$type: COLLECTION.decision,
			request,
			requester: ALICE,
			outcome: 'rejected',
			reason: 'Not a DJ',
			decidedBy: ROOT,
			createdAt: '2026-01-02T00:00:00.000Z'
		});

		await sync();

		expect(await t.requests.get(request)).toMatchObject({
			requester: { did: ALICE },
			status: 'rejected',
			decision: { at: '2026-01-02T00:00:00.000Z', reason: 'Not a DJ' }
		});
	});
});
