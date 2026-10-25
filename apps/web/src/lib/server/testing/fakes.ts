import { XRPCError } from '@atproto/api';
import { TID } from '@atproto/common-web';
import {
	type BlobJson,
	type JsonRecord,
	type RecordRef,
	recordCid,
	type RepoWrite,
	type RootRepoClient,
	type SimpleSpaceConfig,
	type StoredRecord
} from '../atproto/repo-client';
import type { RecordEvent } from './happyview';

/**
 * An in-memory stand-in for a network of PDSes, shared by every fake client
 * so tests can observe what each account wrote where.
 */
export class FakeNetwork {
	/** key: `${space}|${repo}|${collection}|${rkey}` */
	readonly spaceRecords = new Map<string, StoredRecord>();
	/** key: `${repo}|${collection}|${rkey}` */
	readonly records = new Map<string, StoredRecord>();
	readonly spaces = new Map<
		string,
		SimpleSpaceConfig & { uri: string; members: Map<string, { read: boolean; write: boolean }> }
	>();
	readonly blobs = new Map<string, Uint8Array>();
	/** Which repos hold each blob, by CID: a PDS refuses a record whose blobs the writer never uploaded. */
	readonly blobHolders = new Map<string, Set<string>>();
	failures = new Map<string, { count: number; error: XRPCError }>();
	/** Called after every public-repo write, the way HappyView's Jetstream consumer hears of it. */
	onRecord: (event: RecordEvent) => Promise<void> = async () => {};

	client(did: string): FakeRootClient {
		return new FakeRootClient(did, this);
	}

	/** Make the next `count` writes by `did` fail, with `error` (a PDS that's down, by default). */
	failWrites(did: string, count = 1, error = new XRPCError(502, 'UpstreamFailure')): void {
		this.failures.set(did, { count, error });
	}

	spaceRecordsIn(space: string, repo?: string): StoredRecord[] {
		return this.spaceRecords
			.entries()
			.filter(([key]) => key.startsWith(`${space}|`) && (!repo || key.split('|')[1] === repo))
			.map(([, value]) => value)
			.toArray();
	}

	/** Answers public getRecord and listRecords from the public repos, as a PDS does for anyone. */
	readonly publicFetch: typeof fetch = async (input) => {
		const url = new URL(input instanceof Request ? input.url : String(input));
		const repo = url.searchParams.get('repo');
		const collection = url.searchParams.get('collection');
		const prefix = `${repo}|${collection}|`;

		if (url.pathname.endsWith('getRecord')) {
			const found = this.records.get(`${prefix}${url.searchParams.get('rkey')}`);

			return found ? Response.json(found) : Response.json({ error: 'RecordNotFound' }, { status: 400 });
		}

		const records = this.records
			.entries()
			.filter(([key]) => key.startsWith(prefix))
			.map(([, value]) => value)
			.toArray();

		return Response.json({ records });
	};

	/** Throws BlobNotFound, as a PDS does, when `record` references a blob `did` doesn't hold. */
	assertBlobsHeld(did: string, record: JsonRecord): void {
		// Blob refs are the only `$link`s in the records these tests write.
		const missing = JSON.stringify(record)
			.matchAll(/"\$link":"([^"]+)"/g)
			.map(([, cid]) => cid)
			.find((cid) => !this.blobHolders.get(cid)?.has(did));

		if (missing) throw new XRPCError(400, 'BlobNotFound', `Could not find blob: ${missing}`);
	}

	/** Throws, as a PDS does, when a member-list space doesn't let `did` write. */
	assertCanWrite(did: string, space: string): void {
		const found = this.spaces.get(space);

		if (!found || found.writePolicy.$type !== 'com.atproto.simplespace.defs#memberListPolicy') return;

		if (space.startsWith(`at://${did}/`) || found.members.get(did)?.write) return;

		throw new XRPCError(403, 'Forbidden', `${did} can't write to ${space}`);
	}

	maybeFail(did: string): void {
		const failure = this.failures.get(did);

		if (failure && failure.count > 0) {
			failure.count -= 1;
			throw failure.error;
		}
	}
}

export class FakeRootClient implements RootRepoClient {
	constructor(
		readonly did: string,
		private readonly net: FakeNetwork
	) {}

	async createSpaceRecord(input: {
		space: string;
		collection: string;
		record: JsonRecord;
		rkey?: string;
	}): Promise<RecordRef> {
		this.net.maybeFail(this.did);
		const rkey = input.rkey ?? TID.nextStr();
		const key = `${input.space}|${this.did}|${input.collection}|${rkey}`;

		if (this.net.spaceRecords.has(key)) throw new XRPCError(400, 'RecordAlreadyExists');

		return this.writeSpace(key, input.space, input.collection, rkey, input.record);
	}

	async putSpaceRecord(input: {
		space: string;
		collection: string;
		rkey: string;
		record: JsonRecord;
	}): Promise<RecordRef> {
		this.net.maybeFail(this.did);
		const key = `${input.space}|${this.did}|${input.collection}|${input.rkey}`;

		return this.writeSpace(key, input.space, input.collection, input.rkey, input.record);
	}

	private async writeSpace(
		key: string,
		space: string,
		collection: string,
		rkey: string,
		record: JsonRecord
	): Promise<RecordRef> {
		this.net.assertBlobsHeld(this.did, record);
		this.net.assertCanWrite(this.did, space);
		const uri = `${space}/${this.did}/${collection}/${rkey}`;
		const cid = await recordCid(record);
		this.net.spaceRecords.set(key, { uri, cid, value: structuredClone(record) });

		return { uri, cid };
	}

	async deleteSpaceRecord(input: { space: string; collection: string; rkey: string }): Promise<void> {
		this.net.maybeFail(this.did);
		this.net.spaceRecords.delete(`${input.space}|${this.did}|${input.collection}|${input.rkey}`);
	}

	async getSpaceRecord(input: { space: string; repo: string; collection: string; rkey: string }) {
		return this.net.spaceRecords.get(`${input.space}|${input.repo}|${input.collection}|${input.rkey}`) ?? null;
	}

	async listSpaceRecords(input: { space: string; repo: string; collection?: string }) {
		const records = this.net.spaceRecords
			.entries()
			.filter(([key]) => {
				const [space, repo, collection] = key.split('|');

				return space === input.space && repo === input.repo && (!input.collection || collection === input.collection);
			})
			.map(([key, value]) => {
				const [, , collection, rkey] = key.split('|');

				return { collection, rkey, cid: value.cid, value: value.value };
			})
			.toArray();

		return { records };
	}

	async uploadBlob(bytes: Uint8Array, mimeType: string): Promise<BlobJson> {
		const cid = await recordCid({ bytes: Buffer.from(bytes).toString('base64') });
		this.net.blobs.set(cid, bytes);
		this.net.blobHolders.set(cid, (this.net.blobHolders.get(cid) ?? new Set()).add(this.did));

		return { $type: 'blob', ref: { $link: cid }, mimeType, size: bytes.length };
	}

	async createSpace(input: { spaceType: string; skey: string } & SimpleSpaceConfig): Promise<string> {
		this.net.maybeFail(this.did);
		const uri = `at://${this.did}/space/${input.spaceType}/${input.skey}`;

		if (this.net.spaces.has(uri)) throw new XRPCError(400, 'SpaceAlreadyExists');
		this.net.spaces.set(uri, {
			uri,
			readPolicy: input.readPolicy,
			writePolicy: input.writePolicy,
			appAccess: input.appAccess,
			members: new Map()
		});

		return uri;
	}

	async getSpace(space: string) {
		const found = this.net.spaces.get(space);

		if (!found) return null;

		return { uri: found.uri, readPolicy: found.readPolicy, writePolicy: found.writePolicy, appAccess: found.appAccess };
	}

	async updateSpace(input: { space: string } & Partial<SimpleSpaceConfig>): Promise<void> {
		const found = this.net.spaces.get(input.space);

		if (!found) throw new XRPCError(404, 'SpaceNotFound');

		if (input.readPolicy) found.readPolicy = input.readPolicy;

		if (input.writePolicy) found.writePolicy = input.writePolicy;

		if (input.appAccess) found.appAccess = input.appAccess;
	}

	/** The space, if this account owns it; a PDS lets only the owner manage a space. */
	private owned(space: string) {
		const found = this.net.spaces.get(space);

		if (!found) throw new XRPCError(404, 'SpaceNotFound');

		if (!space.startsWith(`at://${this.did}/`)) throw new XRPCError(403, 'Forbidden');

		return found;
	}

	async deleteSpace(space: string): Promise<void> {
		this.owned(space);
		this.net.spaces.delete(space);

		for (const key of this.net.spaceRecords.keys()) if (key.startsWith(`${space}|`)) this.net.spaceRecords.delete(key);
	}

	async putMember(input: { space: string; did: string; read: boolean; write: boolean }): Promise<void> {
		this.net.maybeFail(this.did);
		this.owned(input.space).members.set(input.did, { read: input.read, write: input.write });
	}

	async removeMember(input: { space: string; did: string }): Promise<void> {
		this.net.maybeFail(this.did);
		this.owned(input.space).members.delete(input.did);
	}

	async listMembers(space: string) {
		const found = this.net.spaces.get(space);

		return found ? [...found.members.entries()].map(([did, access]) => ({ did, ...access })) : [];
	}

	async getDelegationToken(): Promise<string> {
		return 'fake-delegation-token';
	}

	async listRecords(collection: string): Promise<StoredRecord[]> {
		const prefix = `${this.did}|${collection}|`;

		return this.net.records
			.entries()
			.filter(([key]) => key.startsWith(prefix))
			.map(([, value]) => value)
			.toArray();
	}

	async listSpaces(spaceType: string): Promise<string[]> {
		const spaces = new Set<string>();

		for (const key of this.net.spaceRecords.keys()) {
			const [space, repo] = key.split('|');

			if (repo === this.did && space.includes(`/space/${spaceType}/`)) spaces.add(space);
		}

		return [...spaces];
	}

	async putRecord(input: {
		collection: string;
		rkey: string;
		record: JsonRecord;
		swapRecord?: string | null;
	}): Promise<RecordRef> {
		this.net.maybeFail(this.did);
		this.net.assertBlobsHeld(this.did, input.record);
		const key = `${this.did}|${input.collection}|${input.rkey}`;
		const existing = this.net.records.get(key);

		if (input.swapRecord !== undefined && (existing?.cid ?? null) !== input.swapRecord) {
			throw new XRPCError(400, 'InvalidSwap');
		}

		const uri = `at://${this.did}/${input.collection}/${input.rkey}`;
		const cid = await recordCid(input.record);
		this.net.records.set(key, { uri, cid, value: structuredClone(input.record) });
		await this.net.onRecord({
			did: this.did,
			collection: input.collection,
			rkey: input.rkey,
			cid,
			record: input.record
		});

		return { uri, cid };
	}

	createRecord(input: { collection: string; record: JsonRecord }): Promise<RecordRef> {
		return this.putRecord({ ...input, rkey: TID.nextStr(), swapRecord: null });
	}

	async getRecord(input: { collection: string; rkey: string }) {
		return this.net.records.get(`${this.did}|${input.collection}|${input.rkey}`) ?? null;
	}

	/** Fails as a whole before writing anything, as the PDS does. */
	async applyWrites(writes: RepoWrite[]): Promise<void> {
		this.net.maybeFail(this.did);

		for (const write of writes) {
			const key = `${this.did}|${write.collection}|${write.rkey}`;

			if (write.action !== 'delete' && this.net.records.has(key) !== (write.action === 'update'))
				throw new XRPCError(400, 'InvalidRequest', `${write.action} of ${key}`);
		}

		for (const write of writes) {
			if (write.action === 'delete') await this.deleteRecord(write);
			else await this.putRecord(write);
		}
	}

	async deleteRecord(input: { collection: string; rkey: string }): Promise<void> {
		this.net.maybeFail(this.did);
		this.net.records.delete(`${this.did}|${input.collection}|${input.rkey}`);
		await this.net.onRecord({ did: this.did, collection: input.collection, rkey: input.rkey, cid: '', record: null });
	}
}
