import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { COLLECTION } from '../../src/lib/nsid.ts';
import { SpaceCredentialManager, XrpcResponseError } from '../../src/lib/server/atproto/credentials.ts';
import { AgentRootClient } from '../../src/lib/server/atproto/repo-client.ts';
import { silentLogger } from '../../src/lib/server/logger.ts';
import { TestNetwork } from './network.ts';

const listRecordsOutput = z.object({ records: z.array(z.object({ rkey: z.string() })) });

describe('staff spaces on a real atproto spaces network', () => {
	let net: TestNetwork;

	beforeAll(async () => {
		net = await TestNetwork.create(['ada', 'milo'], { admins: ['ada'] });
	});
	afterEach(() => {
		vi.restoreAllMocks();
	});
	afterAll(async () => {
		await net?.close();
	});

	/** The staff space's roles, read with a credential just minted for `name`, so one held from earlier can't help. */
	const rolesReadBy = async (name: string) => {
		const credentials = new SpaceCredentialManager({
			source: async () => new AgentRootClient(net.user(name).did, net.user(name).agent),
			identity: net.app.identity,
			logger: silentLogger
		});

		const space = net.app.spaces.staff;

		const { records } = await credentials.query({
			space,
			service: net.pds.url,
			audience: net.rootDid,
			nsid: 'com.atproto.space.listRecords',
			params: { space, repo: net.rootDid, collection: COLLECTION.staffRole },
			schema: listRecordsOutput
		});

		return records.map((record) => record.rkey);
	};

	it('keeps a configured admin in through a reconcile with no roles written yet', async () => {
		await net.app.staff.reconcileMembership();

		await expect(rolesReadBy('ada')).resolves.toEqual([]);
		await expect(net.app.staff.strayMembers()).resolves.toEqual([]);
	});

	it('cuts off a moderator whose removal failed once a reconcile gets through', async () => {
		const ada = await net.viewer('ada');
		const milo = net.user('milo').did;

		await net.app.staff.grant(ada, milo, 'moderator');
		await expect(rolesReadBy('milo')).resolves.toEqual([milo]);

		const removing = vi.spyOn(AgentRootClient.prototype, 'removeMember').mockRejectedValue(new Error('PDS down'));
		await net.app.staff.revoke(ada, milo);
		removing.mockRestore();

		await expect(rolesReadBy('milo')).resolves.toEqual([]);
		await expect(net.app.staff.strayMembers()).resolves.toMatchObject([{ did: milo }]);

		await net.app.staff.reconcileMembership();

		await expect(rolesReadBy('milo')).rejects.toBeInstanceOf(XrpcResponseError);
		await expect(net.app.staff.strayMembers()).resolves.toEqual([]);
	});
});
