import { Agent } from '@atproto/api';
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import {
	AgentRepoClient,
	AgentRootClient,
	type JsonRecord,
	listAllSpaceRecords,
	memberSpacePolicy,
	type RepoClient,
	recordCid,
	xrpcErrorName
} from './repo-client';
import { spaceRecordUri, spaceUri } from './spaces';

const DID = 'did:plc:alice';

const SPACE = spaceUri(DID, 'computer.localnetwork.space.planner', 'self');

const COLLECTION = 'computer.localnetwork.planner.task';

const RECORD: JsonRecord = { $type: COLLECTION, title: 'call the venue' };

type Call = { nsid: string; params: Record<string, string>; body: unknown; contentType: string | null };

const xrpcError = (status: number, error: string) => Response.json({ error, message: error }, { status });

/**
 * A client for `DID` whose agent talks to a PDS that answers each method from `routes` (anything else succeeds with
 * no body), and every call that PDS got.
 */
function pds(routes: Record<string, (call: Call) => Response> = {}) {
	const calls: Call[] = [];

	const agent = new Agent(async (url, init) => {
		const { pathname, searchParams } = new URL(url, 'https://pds.test');
		const nsid = pathname.replace('/xrpc/', '');
		const contentType = new Headers(init.headers).get('content-type');
		const isJson = contentType === 'application/json';
		const body = isJson ? await new Response(init.body).json() : init.body;
		const call = { nsid, params: Object.fromEntries(searchParams), body, contentType };
		calls.push(call);

		return routes[nsid]?.(call) ?? new Response(null);
	});

	return { calls, client: new AgentRepoClient(DID, agent), root: new AgentRootClient(DID, agent) };
}

describe('AgentRepoClient', () => {
	it('writes records in a space and in the public repo as its own account, and returns their refs', async () => {
		const cid = await recordCid(RECORD);

		const ref = (call: Call) => {
			const { collection, rkey } = z
				.object({ collection: z.string(), rkey: z.string().default('tid1') })
				.parse(call.body);

			return Response.json({ uri: spaceRecordUri(SPACE, DID, collection, rkey), cid });
		};

		const { calls, client } = pds({
			'com.atproto.space.createRecord': ref,
			'com.atproto.space.putRecord': ref,
			'com.atproto.repo.createRecord': () => Response.json({ uri: `at://${DID}/${COLLECTION}/tid2`, cid }),
			'com.atproto.repo.putRecord': () => Response.json({ uri: `at://${DID}/${COLLECTION}/main`, cid })
		});

		expect(await client.createSpaceRecord({ space: SPACE, collection: COLLECTION, record: RECORD })).toEqual({
			uri: spaceRecordUri(SPACE, DID, COLLECTION, 'tid1'),
			cid
		});
		expect(await client.putSpaceRecord({ space: SPACE, collection: COLLECTION, rkey: 'b', record: RECORD })).toEqual({
			uri: spaceRecordUri(SPACE, DID, COLLECTION, 'b'),
			cid
		});
		await client.deleteSpaceRecord({ space: SPACE, collection: COLLECTION, rkey: 'b' });
		expect(await client.createRecord({ collection: COLLECTION, record: RECORD })).toEqual({
			uri: `at://${DID}/${COLLECTION}/tid2`,
			cid
		});
		expect(await client.putRecord({ collection: COLLECTION, rkey: 'main', record: RECORD, swapRecord: cid })).toEqual({
			uri: `at://${DID}/${COLLECTION}/main`,
			cid
		});
		await client.deleteRecord({ collection: COLLECTION, rkey: 'main' });

		expect(calls.map(({ nsid, body }) => [nsid, body])).toEqual([
			['com.atproto.space.createRecord', { space: SPACE, repo: DID, collection: COLLECTION, record: RECORD }],
			['com.atproto.space.putRecord', { space: SPACE, repo: DID, collection: COLLECTION, rkey: 'b', record: RECORD }],
			['com.atproto.space.deleteRecord', { space: SPACE, repo: DID, collection: COLLECTION, rkey: 'b' }],
			['com.atproto.repo.createRecord', { repo: DID, collection: COLLECTION, record: RECORD }],
			[
				'com.atproto.repo.putRecord',
				{ repo: DID, collection: COLLECTION, rkey: 'main', record: RECORD, swapRecord: cid }
			],
			['com.atproto.repo.deleteRecord', { repo: DID, collection: COLLECTION, rkey: 'main' }]
		]);
	});

	it("sends a batch to a space or the public repo typed with that method's own write types", async () => {
		const { calls, client } = pds();

		const writes = [
			{ action: 'create', collection: COLLECTION, rkey: 'a', record: RECORD },
			{ action: 'update', collection: COLLECTION, rkey: 'b', record: RECORD },
			{ action: 'delete', collection: COLLECTION, rkey: 'c' }
		] as const;

		const typed = (nsid: string) => [
			{ $type: `${nsid}#create`, collection: COLLECTION, rkey: 'a', value: RECORD },
			{ $type: `${nsid}#update`, collection: COLLECTION, rkey: 'b', value: RECORD },
			{ $type: `${nsid}#delete`, collection: COLLECTION, rkey: 'c' }
		];

		await client.applySpaceWrites(SPACE, [...writes]);
		await client.applyWrites([...writes]);

		expect(calls.map(({ body }) => body)).toEqual([
			{ space: SPACE, repo: DID, writes: typed('com.atproto.space.applyWrites') },
			{ repo: DID, writes: typed('com.atproto.repo.applyWrites') }
		]);
	});

	it('reads records back as JSON, blob refs included, whether in a space or the public repo', async () => {
		const blobCid = await recordCid({ bytes: 'image' });

		const value = {
			$type: COLLECTION,
			cover: { $type: 'blob', ref: { $link: blobCid }, mimeType: 'image/png', size: 5 }
		};

		const cid = await recordCid(value);
		const uri = spaceRecordUri(SPACE, DID, COLLECTION, 'a');

		const { calls, client } = pds({
			'com.atproto.space.getRecord': () => Response.json({ uri, cid, value }),
			// A public read may leave out the CID. Tests RepoClient's own read, which app code may only make where allowed.
			// ast-grep-ignore: no-direct-pds-read
			'com.atproto.repo.getRecord': () => Response.json({ uri: `at://${DID}/${COLLECTION}/a`, value })
		});

		expect(await client.getSpaceRecord({ space: SPACE, repo: DID, collection: COLLECTION, rkey: 'a' })).toEqual({
			uri,
			cid,
			value
		});
		// ast-grep-ignore: no-direct-pds-read
		expect(await client.getRecord({ collection: COLLECTION, rkey: 'a' })).toEqual({
			uri: `at://${DID}/${COLLECTION}/a`,
			cid: '',
			value
		});
		expect(calls[1].params).toEqual({ repo: DID, collection: COLLECTION, rkey: 'a' });
	});

	it.each([
		[
			'getSpaceRecord',
			'com.atproto.space.getRecord',
			(client: RepoClient) => client.getSpaceRecord({ space: SPACE, repo: DID, collection: COLLECTION, rkey: 'a' })
		],
		['getSpace', 'com.atproto.space.getSpace', (client: RepoClient) => client.getSpace(SPACE)],
		[
			'getRecord',
			// Tests RepoClient's own read, as above.
			// ast-grep-ignore: no-direct-pds-read
			'com.atproto.repo.getRecord',
			// ast-grep-ignore: no-direct-pds-read
			(client: RepoClient) => client.getRecord({ collection: COLLECTION, rkey: 'a' })
		]
	] as const)(
		'%s reads a missing record, repo, or space as null, and throws any other failure',
		async (_name, nsid, read) => {
			const failingWith = (response: Response) => read(pds({ [nsid]: () => response }).client);

			for (const error of ['RecordNotFound', 'SpaceNotFound', 'RepoNotFound']) {
				expect(await failingWith(xrpcError(400, error))).toBeNull();
			}

			expect(await failingWith(xrpcError(404, 'NotFound'))).toBeNull();

			await expect(failingWith(xrpcError(400, 'InvalidRequest'))).rejects.toMatchObject({ error: 'InvalidRequest' });
			await expect(failingWith(xrpcError(500, 'InternalServerError'))).rejects.toMatchObject({
				error: 'InternalServerError'
			});
		}
	);

	it('lists 100 space records a page by default, reading a record listed without its value as empty', async () => {
		const cid = await recordCid(RECORD);

		const { calls, client } = pds({
			'com.atproto.space.listRecords': () =>
				Response.json({
					cursor: 'next',
					records: [
						{ collection: COLLECTION, rkey: 'a', cid, value: RECORD },
						{ collection: COLLECTION, rkey: 'b', cid }
					]
				})
		});

		expect(await client.listSpaceRecords({ space: SPACE, repo: DID, cursor: 'start' })).toEqual({
			cursor: 'next',
			records: [
				{ collection: COLLECTION, rkey: 'a', cid, value: RECORD },
				{ collection: COLLECTION, rkey: 'b', cid, value: {} }
			]
		});
		await client.listSpaceRecords({ space: SPACE, repo: DID, collection: COLLECTION, limit: 5 });

		expect(calls.map(({ params }) => params)).toEqual([
			{ space: SPACE, repo: DID, cursor: 'start', limit: '100' },
			{ space: SPACE, repo: DID, collection: COLLECTION, limit: '5' }
		]);
	});

	it('uploads a blob with its MIME type and returns its JSON ref', async () => {
		const bytes = new TextEncoder().encode('image');
		const link = await recordCid({ bytes: 'image' });

		const { calls, client } = pds({
			'com.atproto.repo.uploadBlob': () =>
				Response.json({
					blob: { $type: 'blob', ref: { $link: link }, mimeType: 'image/webp', size: bytes.length }
				})
		});

		expect(await client.uploadBlob(bytes, 'image/webp')).toEqual({
			$type: 'blob',
			ref: { $link: link },
			mimeType: 'image/webp',
			size: 5
		});
		expect(calls[0]).toMatchObject({ body: bytes, contentType: 'image/webp' });
	});

	it('creates, reads, and deletes a space, and manages its members', async () => {
		const policy = memberSpacePolicy({ publicWrite: true });

		const { calls, client } = pds({
			'com.atproto.space.createSpace': () => Response.json({ uri: SPACE }),
			'com.atproto.space.getSpace': () => Response.json({ uri: SPACE, policy })
		});

		expect(await client.createSpace({ spaceType: 'computer.localnetwork.space.planner', skey: 'self', policy })).toBe(
			SPACE
		);
		expect(await client.getSpace(SPACE)).toEqual({ uri: SPACE, policy });
		await client.putMember({ space: SPACE, did: 'did:plc:bob', read: true, write: false });
		await client.removeMember({ space: SPACE, did: 'did:plc:bob' });
		await client.deleteSpace(SPACE);

		expect(calls.map(({ nsid, params, body }) => [nsid, body ?? params])).toEqual([
			['com.atproto.space.createSpace', { spaceType: 'computer.localnetwork.space.planner', skey: 'self', policy }],
			['com.atproto.space.getSpace', { space: SPACE }],
			['com.atproto.memberspace.putMember', { space: SPACE, did: 'did:plc:bob', read: true, write: false }],
			['com.atproto.memberspace.removeMember', { space: SPACE, did: 'did:plc:bob' }],
			['com.atproto.space.deleteSpace', { space: SPACE }]
		]);
	});

	it('lists every member of a space, page by page', async () => {
		const { calls, client } = pds({
			'com.atproto.memberspace.listMembers': ({ params }) =>
				Response.json(
					params.cursor
						? { members: [{ did: 'did:plc:carol', read: true, write: false }] }
						: { members: [{ did: 'did:plc:bob', read: true, write: true }], cursor: 'p2' }
				)
		});

		expect(await client.listMembers(SPACE)).toEqual([
			{ did: 'did:plc:bob', read: true, write: true },
			{ did: 'did:plc:carol', read: true, write: false }
		]);
		expect(calls.map(({ params }) => params)).toEqual([
			{ space: SPACE, limit: '100' },
			{ space: SPACE, limit: '100', cursor: 'p2' }
		]);
	});
});

describe('AgentRootClient', () => {
	it("replaces a space's policy and fetches its delegation token", async () => {
		const policy = memberSpacePolicy();
		const { calls, root } = pds({ 'com.atproto.space.getDelegationToken': () => Response.json({ token: 'jwt' }) });

		await root.updateSpace({ space: SPACE, policy });
		expect(await root.getDelegationToken(SPACE)).toBe('jwt');
		expect(calls.map(({ body, params }) => body ?? params)).toEqual([{ space: SPACE, policy }, { space: SPACE }]);
	});

	it('lists every record of a public collection, page by page', async () => {
		const cid = await recordCid(RECORD);
		const record = (rkey: string) => ({ uri: `at://${DID}/${COLLECTION}/${rkey}`, cid, value: RECORD });

		const { calls, root } = pds({
			// Tests RepoClient's own read, as above.
			// ast-grep-ignore: no-direct-pds-read
			'com.atproto.repo.listRecords': ({ params }) =>
				Response.json(params.cursor ? { records: [record('b')] } : { records: [record('a')], cursor: 'p2' })
		});

		// ast-grep-ignore: no-direct-pds-read
		expect(await root.listRecords(COLLECTION)).toEqual([record('a'), record('b')]);
		expect(calls.map(({ params }) => params)).toEqual([
			{ repo: DID, collection: COLLECTION, limit: '100' },
			{ repo: DID, collection: COLLECTION, limit: '100', cursor: 'p2' }
		]);
	});

	it('lists every space of a type it holds a repo in, page by page', async () => {
		const other = spaceUri(DID, 'computer.localnetwork.space.booking', 'b');

		const { calls, root } = pds({
			'com.atproto.space.listSpaces': ({ params }) =>
				Response.json(params.cursor ? { spaces: [{ uri: other }] } : { spaces: [{ uri: SPACE }], cursor: 'p2' })
		});

		expect(await root.listSpaces('computer.localnetwork.space.booking')).toEqual([SPACE, other]);
		expect(calls.map(({ params }) => params)).toEqual([
			{ spaceType: 'computer.localnetwork.space.booking', did: DID, limit: '100' },
			{ spaceType: 'computer.localnetwork.space.booking', did: DID, limit: '100', cursor: 'p2' }
		]);
	});
});

describe('listAllSpaceRecords', () => {
	it('reads every page in order until one comes back without a cursor', async () => {
		const cid = await recordCid(RECORD);
		const record = (rkey: string) => ({ collection: COLLECTION, rkey, cid, value: RECORD });

		// Each page's cursor is the index of the next one.
		const pages = [
			{ records: [record('a'), record('b')], cursor: '1' },
			{ records: [record('c')], cursor: '2' },
			{ records: [record('d')] }
		];

		const { calls, client } = pds({
			'com.atproto.space.listRecords': ({ params }) => Response.json(pages[Number(params.cursor ?? 0)])
		});

		const input = { space: SPACE, repo: DID, collection: COLLECTION };
		expect((await listAllSpaceRecords(client, input)).map(({ rkey }) => rkey)).toEqual(['a', 'b', 'c', 'd']);
		expect(calls.map(({ params }) => params.cursor)).toEqual([undefined, '1', '2']);
	});
});

describe('xrpcErrorName', () => {
	it('names only XRPC errors', () => {
		expect(xrpcErrorName(new Error('RecordNotFound'))).toBeUndefined();
	});
});
