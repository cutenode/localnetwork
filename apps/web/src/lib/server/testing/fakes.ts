import { XRPCError } from '@atproto/api';
import { TID } from '@atproto/common-web';
import { cidForLex } from '@atproto/lex-cbor';
import { type JsonValue, jsonToLex } from '@atproto/lex-json';
import type {
	BlobJson,
	JsonRecord,
	RecordRef,
	RootRepoClient,
	SimpleSpaceConfig,
	StoredRecord
} from '../atproto/repo-client';
import type { RecordEvent } from './happyview';

const cidOf = async (value: JsonValue): Promise<string> => (await cidForLex(jsonToLex(value))).toString();

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
	failures = new Map<string, number>();
	/** Called after every public-repo write, the way HappyView's Jetstream consumer hears of it. */
	onRecord: (event: RecordEvent) => Promise<void> = async () => {};

	client(did: string): FakeRootClient {
		return new FakeRootClient(did, this);
	}

	/** Make the next `count` writes by `did` fail. */
	failWrites(did: string, count = 1): void {
		this.failures.set(did, count);
	}

	spaceRecordsIn(space: string, repo?: string): StoredRecord[] {
		return this.spaceRecords
			.entries()
			.filter(([key]) => key.startsWith(`${space}|`) && (!repo || key.split('|')[1] === repo))
			.map(([, value]) => value)
			.toArray();
	}

	maybeFail(did: string): void {
		const remaining = this.failures.get(did) ?? 0;

		if (remaining > 0) {
			this.failures.set(did, remaining - 1);
			throw new XRPCError(502, 'UpstreamFailure');
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
		const uri = `${space}/${this.did}/${collection}/${rkey}`;
		const cid = await cidOf(record);
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
		const cid = await cidOf({ bytes: Buffer.from(bytes).toString('base64') });
		this.net.blobs.set(cid, bytes);

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

	async putMember(input: { space: string; did: string; read: boolean; write: boolean }): Promise<void> {
		const found = this.net.spaces.get(input.space);

		if (!found) throw new XRPCError(404, 'SpaceNotFound');
		found.members.set(input.did, { read: input.read, write: input.write });
	}

	async removeMember(input: { space: string; did: string }): Promise<void> {
		this.net.spaces.get(input.space)?.members.delete(input.did);
	}

	async listMembers(space: string) {
		const found = this.net.spaces.get(space);

		return found ? [...found.members.entries()].map(([did, access]) => ({ did, ...access })) : [];
	}

	async getDelegationToken(): Promise<string> {
		return 'fake-delegation-token';
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
		const key = `${this.did}|${input.collection}|${input.rkey}`;
		const existing = this.net.records.get(key);

		if (input.swapRecord !== undefined && (existing?.cid ?? null) !== input.swapRecord) {
			throw new XRPCError(400, 'InvalidSwap');
		}

		const uri = `at://${this.did}/${input.collection}/${input.rkey}`;
		const cid = await cidOf(input.record);
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

	async getRecord(input: { collection: string; rkey: string }) {
		return this.net.records.get(`${this.did}|${input.collection}|${input.rkey}`) ?? null;
	}

	async deleteRecord(input: { collection: string; rkey: string }): Promise<void> {
		this.net.records.delete(`${this.did}|${input.collection}|${input.rkey}`);
	}
}
