import { type Agent, type ComAtprotoRepoGetRecord, type ComAtprotoSimplespaceGetSpace, XRPCError } from '@atproto/api';
import { cidForLex } from '@atproto/lex-cbor';
import { lexToJson } from '@atproto/lexicon';
import { type JsonObject, jsonToLex } from '@atproto/lex-json';
import { z } from 'zod';

export type JsonRecord = JsonObject;

export type RecordRef = { uri: string; cid: string };

export type StoredRecord = { uri: string; cid: string; value: JsonRecord };

export type ListedSpaceRecord = { collection: string; rkey: string; cid: string; value: JsonRecord };

/** One write in an applyWrites batch to an account's own public repo, or to its repo in a space. */
export type RepoWrite =
	| { action: 'create' | 'update'; collection: string; rkey: string; record: JsonRecord }
	| { action: 'delete'; collection: string; rkey: string };

export type BlobJson = { $type: 'blob'; ref: { $link: string }; mimeType: string; size: number };

export type SimpleSpacePolicy =
	| { $type: 'com.atproto.simplespace.defs#publicPolicy' }
	| { $type: 'com.atproto.simplespace.defs#memberListPolicy' }
	| { $type: 'com.atproto.simplespace.defs#managingAppPolicy'; managingApp: string };

export type SimpleSpaceConfig = {
	readPolicy: SimpleSpacePolicy;
	writePolicy: SimpleSpacePolicy;
	appAccess:
		| { $type: 'com.atproto.simplespace.defs#open' }
		| { $type: 'com.atproto.simplespace.defs#allowList'; allowed: string[] };
};

/** A space as its host reports it. The policy unions stay open, since a host may know kinds this app doesn't. */
export type SimpleSpaceView = ComAtprotoSimplespaceGetSpace.OutputSchema;

/**
 * The narrow set of repo and space operations the app performs as a given
 * account. Production uses {@link AgentRepoClient}; tests use in-memory fakes.
 */
export interface RepoClient {
	readonly did: string;
	createSpaceRecord(input: {
		space: string;
		collection: string;
		record: JsonRecord;
		rkey?: string;
	}): Promise<RecordRef>;
	putSpaceRecord(input: { space: string; collection: string; rkey: string; record: JsonRecord }): Promise<RecordRef>;
	deleteSpaceRecord(input: { space: string; collection: string; rkey: string }): Promise<void>;
	/**
	 * Several writes to the account's repo in a space, made all together or not at all. A create fails on a record that
	 * exists, and an update or delete on one that doesn't.
	 */
	applySpaceWrites(space: string, writes: RepoWrite[]): Promise<void>;
	getSpaceRecord(input: {
		space: string;
		repo: string;
		collection: string;
		rkey: string;
	}): Promise<StoredRecord | null>;
	listSpaceRecords(input: {
		space: string;
		repo: string;
		collection?: string;
		cursor?: string;
		limit?: number;
	}): Promise<{ records: ListedSpaceRecord[]; cursor?: string }>;
	uploadBlob(bytes: Uint8Array, mimeType: string): Promise<BlobJson>;
	/** Records in the account's own public repo. */
	putRecord(input: {
		collection: string;
		rkey: string;
		record: JsonRecord;
		swapRecord?: string | null;
	}): Promise<RecordRef>;
	/** A new record in the account's own public repo, under a fresh key: unlike putRecord, it needs no update permission. */
	createRecord(input: { collection: string; record: JsonRecord }): Promise<RecordRef>;
	getRecord(input: { collection: string; rkey: string }): Promise<StoredRecord | null>;
	/** Several writes to the account's own public repo, made all together or not at all. */
	applyWrites(writes: RepoWrite[]): Promise<void>;
	deleteRecord(input: { collection: string; rkey: string }): Promise<void>;
	/** Simplespace management for spaces under the account's own DID. */
	createSpace(input: { spaceType: string; skey: string } & SimpleSpaceConfig): Promise<string>;
	getSpace(space: string): Promise<SimpleSpaceView | null>;
	deleteSpace(space: string): Promise<void>;
	putMember(input: { space: string; did: string; read: boolean; write: boolean }): Promise<void>;
	removeMember(input: { space: string; did: string }): Promise<void>;
	listMembers(space: string): Promise<{ did: string; read: boolean; write: boolean }[]>;
}

/** Extra operations only the root account performs. */
export interface RootRepoClient extends RepoClient {
	updateSpace(input: { space: string } & Partial<SimpleSpaceConfig>): Promise<void>;
	getDelegationToken(space: string): Promise<string>;
	/** Spaces the root holds a repo in (used to rediscover booking spaces). */
	listSpaces(spaceType: string): Promise<string[]>;
	/** Every record in one collection of the root's own public repo. Unlike a public read, a PDS that's down throws. */
	listRecords(collection: string): Promise<StoredRecord[]>;
}

/** The CID a PDS gives `record` (with its `$type`) when it writes it. */
export async function recordCid(record: JsonRecord): Promise<string> {
	return (await cidForLex(jsonToLex(record))).toString();
}

/** The XRPC error name of a failed {@link RepoClient} call. Implementations reject with `XRPCError`. */
export function xrpcErrorName(cause: unknown): string | undefined {
	return cause instanceof XRPCError ? cause.error : undefined;
}

/**
 * Creates a space, resolving false when it already exists. Callers look it up
 * first, but two requests can both find it missing; the host decides which one
 * created it.
 */
export async function createSpaceIfMissing(
	client: RepoClient,
	input: { spaceType: string; skey: string } & SimpleSpaceConfig
): Promise<boolean> {
	try {
		await client.createSpace(input);

		return true;
	} catch (error) {
		if (xrpcErrorName(error) === 'SpaceAlreadyExists') return false;

		throw error;
	}
}

const NOT_FOUND = new Set(['RecordNotFound', 'SpaceNotFound', 'RepoNotFound']);

// XRPCError types `status` as its ResponseType enum, where an HTTP 404 is the member `XRPCNotSupported` (404).
const isNotFound = (cause: unknown) =>
	cause instanceof XRPCError && (NOT_FOUND.has(cause.error) || Number(cause.status) === 404);

/** A record value as the agent decodes it: lex form, with CIDs and `BlobRef`s. */
type AgentRecord = ComAtprotoRepoGetRecord.OutputSchema['value'];

const jsonRecord = z.record(z.string(), z.json());

const asJson = (value: AgentRecord): JsonRecord => jsonRecord.parse(lexToJson(value));

/** `writes` as the `writes` of `nsid`, the applyWrites of a public repo or of a space, which take the same shape. */
const batch = <N extends 'com.atproto.repo.applyWrites' | 'com.atproto.space.applyWrites'>(
	nsid: N,
	writes: RepoWrite[]
) =>
	writes.map((write) =>
		write.action === 'delete'
			? { $type: `${nsid}#delete` as const, collection: write.collection, rkey: write.rkey }
			: {
					$type: `${nsid}#${write.action}` as const,
					collection: write.collection,
					rkey: write.rkey,
					value: write.record
				}
	);

export class AgentRepoClient implements RepoClient {
	constructor(
		readonly did: string,
		protected readonly agent: Agent
	) {}

	async createSpaceRecord(input: { space: string; collection: string; record: JsonRecord; rkey?: string }) {
		const { data } = await this.agent.com.atproto.space.createRecord({
			space: input.space,
			repo: this.did,
			collection: input.collection,
			rkey: input.rkey,
			record: input.record
		});

		return { uri: data.uri, cid: data.cid };
	}

	async putSpaceRecord(input: { space: string; collection: string; rkey: string; record: JsonRecord }) {
		const { data } = await this.agent.com.atproto.space.putRecord({
			space: input.space,
			repo: this.did,
			collection: input.collection,
			rkey: input.rkey,
			record: input.record
		});

		return { uri: data.uri, cid: data.cid };
	}

	async deleteSpaceRecord(input: { space: string; collection: string; rkey: string }) {
		await this.agent.com.atproto.space.deleteRecord({ ...input, repo: this.did });
	}

	async applySpaceWrites(space: string, writes: RepoWrite[]) {
		await this.agent.com.atproto.space.applyWrites({
			space,
			repo: this.did,
			writes: batch('com.atproto.space.applyWrites', writes)
		});
	}

	async getSpaceRecord(input: { space: string; repo: string; collection: string; rkey: string }) {
		try {
			const { data } = await this.agent.com.atproto.space.getRecord(input);

			return { uri: data.uri, cid: data.cid, value: asJson(data.value) };
		} catch (error) {
			if (isNotFound(error)) return null;
			throw error;
		}
	}

	async listSpaceRecords(input: { space: string; repo: string; collection?: string; cursor?: string; limit?: number }) {
		const { data } = await this.agent.com.atproto.space.listRecords({ limit: 100, ...input });

		return {
			cursor: data.cursor,
			records: data.records.map((record) => ({
				collection: record.collection,
				rkey: record.rkey,
				cid: record.cid,
				value: asJson(record.value ?? {})
			}))
		};
	}

	async uploadBlob(bytes: Uint8Array, mimeType: string): Promise<BlobJson> {
		const { data } = await this.agent.com.atproto.repo.uploadBlob(bytes, { encoding: mimeType });

		return {
			$type: 'blob',
			ref: { $link: data.blob.ref.toString() },
			mimeType: data.blob.mimeType,
			size: data.blob.size
		};
	}

	async createSpace(input: { spaceType: string; skey: string } & SimpleSpaceConfig) {
		const { data } = await this.agent.com.atproto.simplespace.createSpace(input);

		return data.uri;
	}

	async getSpace(space: string) {
		try {
			const { data } = await this.agent.com.atproto.simplespace.getSpace({ space });

			return data;
		} catch (error) {
			if (isNotFound(error)) return null;
			throw error;
		}
	}

	async createRecord(input: { collection: string; record: JsonRecord }) {
		const { data } = await this.agent.com.atproto.repo.createRecord({ ...input, repo: this.did });

		return { uri: data.uri, cid: data.cid };
	}

	async applyWrites(writes: RepoWrite[]) {
		await this.agent.com.atproto.repo.applyWrites({
			repo: this.did,
			writes: batch('com.atproto.repo.applyWrites', writes)
		});
	}

	async putRecord(input: { collection: string; rkey: string; record: JsonRecord; swapRecord?: string | null }) {
		const { data } = await this.agent.com.atproto.repo.putRecord({
			repo: this.did,
			collection: input.collection,
			rkey: input.rkey,
			record: input.record,
			swapRecord: input.swapRecord
		});

		return { uri: data.uri, cid: data.cid };
	}

	async getRecord(input: { collection: string; rkey: string }) {
		try {
			// The account's own repo; callers are checked where they read (see no-direct-pds-read).
			// ast-grep-ignore: no-direct-pds-read
			const { data } = await this.agent.com.atproto.repo.getRecord({ repo: this.did, ...input });

			return { uri: data.uri, cid: data.cid ?? '', value: asJson(data.value) };
		} catch (error) {
			if (isNotFound(error)) return null;
			throw error;
		}
	}

	async deleteRecord(input: { collection: string; rkey: string }) {
		await this.agent.com.atproto.repo.deleteRecord({ repo: this.did, ...input });
	}

	async deleteSpace(space: string) {
		await this.agent.com.atproto.simplespace.deleteSpace({ space });
	}

	async putMember(input: { space: string; did: string; read: boolean; write: boolean }) {
		await this.agent.com.atproto.simplespace.putMember(input);
	}

	async removeMember(input: { space: string; did: string }) {
		await this.agent.com.atproto.simplespace.removeMember(input);
	}

	async listMembers(space: string) {
		const members: { did: string; read: boolean; write: boolean }[] = [];
		let cursor: string | undefined;

		do {
			const { data } = await this.agent.com.atproto.simplespace.listMembers({ space, limit: 100, cursor });
			members.push(...data.members.map(({ did, read, write }) => ({ did, read, write })));
			cursor = data.cursor;
		} while (cursor);

		return members;
	}
}

export class AgentRootClient extends AgentRepoClient implements RootRepoClient {
	async updateSpace(input: { space: string } & Partial<SimpleSpaceConfig>) {
		await this.agent.com.atproto.simplespace.updateSpace(input);
	}

	async getDelegationToken(space: string) {
		const { data } = await this.agent.com.atproto.space.getDelegationToken({ space });

		return data.token;
	}

	async listRecords(collection: string) {
		const records: StoredRecord[] = [];
		let cursor: string | undefined;

		do {
			// The root's own repo; callers are checked where they read (see no-direct-pds-read).
			// ast-grep-ignore: no-direct-pds-read
			const { data } = await this.agent.com.atproto.repo.listRecords({
				repo: this.did,
				collection,
				limit: 100,
				cursor
			});

			records.push(
				...data.records.map((record) => ({ uri: record.uri, cid: record.cid, value: asJson(record.value) }))
			);
			cursor = data.cursor;
		} while (cursor);

		return records;
	}

	async listSpaces(spaceType: string) {
		const spaces: string[] = [];
		let cursor: string | undefined;

		do {
			const { data } = await this.agent.com.atproto.space.listSpaces({ spaceType, did: this.did, limit: 100, cursor });
			spaces.push(...data.spaces.map((space) => space.uri));
			cursor = data.cursor;
		} while (cursor);

		return spaces;
	}
}
