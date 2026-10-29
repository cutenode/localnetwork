import { describe, expect, it } from 'vitest';
import type { RepoWrite } from '../atproto/repo-client';
import { FakeNetwork } from './fakes';

const ROOT = 'did:plc:root';

const policy = { $type: 'com.atproto.simplespace.defs#memberListPolicy' } as const;

describe('FakeRootClient.applySpaceWrites', () => {
	const setup = async () => {
		const net = new FakeNetwork();
		const root = net.client(ROOT);

		const space = await root.createSpace({
			spaceType: 'computer.localnetwork.space.party',
			skey: '3k',
			readPolicy: policy,
			writePolicy: policy,
			appAccess: { $type: 'com.atproto.simplespace.defs#open' }
		});

		const keys = () => net.spaceRecordsIn(space).map((record) => record.uri.split('/').slice(-2).join('/'));

		return { net, root, space, keys };
	};

	it('writes a batch whose writes build on each other, as the PDS does', async () => {
		const { root, space, keys } = await setup();

		await root.applySpaceWrites(space, [
			{ action: 'create', collection: 'a.b.c', rkey: '1', record: { n: 1 } },
			{ action: 'update', collection: 'a.b.c', rkey: '1', record: { n: 2 } },
			{ action: 'create', collection: 'a.b.c', rkey: '2', record: { n: 1 } },
			{ action: 'delete', collection: 'a.b.c', rkey: '2' }
		]);

		expect(keys()).toEqual(['a.b.c/1']);
	});

	it('writes nothing when any write in the batch fails', async () => {
		const { root, space, keys } = await setup();
		await root.applySpaceWrites(space, [{ action: 'create', collection: 'a.b.c', rkey: '1', record: {} }]);

		const fails = (write: RepoWrite, error: string) =>
			expect(
				root.applySpaceWrites(space, [{ action: 'create', collection: 'a.b.c', rkey: '9', record: {} }, write])
			).rejects.toMatchObject({ error });

		await fails({ action: 'create', collection: 'a.b.c', rkey: '1', record: {} }, 'RecordAlreadyExists');
		await fails({ action: 'update', collection: 'a.b.c', rkey: '2', record: {} }, 'RecordNotFound');
		await fails({ action: 'delete', collection: 'a.b.c', rkey: '2' }, 'RecordNotFound');
		expect(keys()).toEqual(['a.b.c/1']);

		await expect(
			root.applySpaceWrites(`at://${ROOT}/space/computer.localnetwork.space.party/gone`, [])
		).rejects.toMatchObject({ error: 'SpaceNotFound' });
	});
});
