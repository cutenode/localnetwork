import { TID } from '@atproto/common-web';
import { P256Keypair } from '@atproto/crypto';
import type { Cid, LexMap } from '@atproto/lex-data';
import { lexStringify } from '@atproto/lex-json';
import { isDidString, isNsidString, isSpaceKeyString } from '@atproto/oauth-scopes';
import {
	COMMIT_MAC_TYPE,
	type Commit,
	computeCommitMac,
	computeRecordMac,
	createSpaceToken,
	decapsulate,
	formatMacHeader,
	isP256DidKey,
	MAC_HEADER,
	type MacType,
	parseSpaceToken,
	parseWantMacHeader,
	RECORD_MAC_TYPE,
	type RecordPathParts,
	RepoCommit,
	serializeRecord,
	serializeRepo,
	verifyCredentialSig,
	WANT_MAC_HEADER
} from '@atproto/space';
import { z } from 'zod';
import { IdentityResolver, type ResolvedIdentity } from '../atproto/identity';
import { parseSpaceUri, spaceRecordUri } from '../atproto/spaces';
import { silentLogger } from '../logger';

type HostedRecord = RecordPathParts & { cid: Cid; bytes: Uint8Array; value: LexMap };

type HostedOp = RecordPathParts & { cid: string | null; prev: string | null };

type HostedRepo = {
	kem: P256Keypair;
	rev: string;
	/** By `collection/rkey`. */
	records: Map<string, HostedRecord>;
	ops: (HostedOp & { rev: string })[];
};

const credentialRequest = z.object({ space: z.string(), credentialKid: z.string(), delegationToken: z.string() });

const xrpcError = (status: number, error: string) => Response.json({ error }, { status });

const isSpaceRef = (value: string): value is Commit['space'] => parseSpaceUri(value) !== null;

/** Resolves every account to the host and the KEM key it holds for them, as a DID document cache would. */
class HostIdentity extends IdentityResolver {
	/** KEM keys cached from before a rotation, served until a forced refresh. */
	readonly staleKemKeys = new Map<string, string>();
	/** Accounts whose DID documents publish no KEM key. */
	readonly withoutKemKeys = new Set<string>();
	/** DIDs resolved with a forced refresh, in order. */
	readonly refreshed: string[] = [];

	constructor(private readonly host: FakeSpaceHost) {
		super({ plcUrl: 'https://plc.test', allowPrivateNetwork: true, logger: silentLogger });
	}

	override async resolve(did: string, forceRefresh = false): Promise<ResolvedIdentity> {
		if (forceRefresh) {
			this.refreshed.push(did);
			this.staleKemKeys.delete(did);
		}

		const kemKey = this.withoutKemKeys.has(did) ? null : (this.staleKemKeys.get(did) ?? this.host.kemKey(did));

		return { did, handle: null, pds: this.host.url, signingKey: '', kemKey };
	}
}

/**
 * One space's authority and the repo host of all its writers, speaking the spaces protocol over `fetch` as
 * the alpha PDS does: credentials are asked for with a JSON body, every other request has to carry one in
 * a signed `Atproto-Space-Credential` header, and listRepoOps, getRepo, and getRecord answer with a MAC
 * keyed to the author's KEM key.
 */
export class FakeSpaceHost {
	readonly url = 'https://host.test';
	/** Every request it got, in order. */
	readonly requests: { nsid: string; headers: Headers; body: unknown }[] = [];
	/** Ops per listRepoOps page, whatever the limit asked for. */
	pageSize = 500;
	/** An error getSpaceCredential answers with, as an authority does for a deleted space or a bad token. */
	credentialError: string | null = null;
	/** Credentials it has revoked, which it answers CredentialRevoked to. */
	readonly revoked = new Set<string>();
	/** Every credential it issued, in order. */
	readonly issued: string[] = [];
	/** MACs responses with a secret of its own instead of the one it shares with the syncer. */
	forgeMacs = false;
	readonly identity = new HostIdentity(this);
	private readonly repos = new Map<string, HostedRepo>();

	private constructor(
		readonly space: Commit['space'],
		private readonly authorityKey: P256Keypair
	) {}

	static async create(space: string): Promise<FakeSpaceHost> {
		if (!isSpaceRef(space)) throw new Error(`Not a space URI: ${space}`);

		return new FakeSpaceHost(space, await P256Keypair.create());
	}

	/** The did:key of `did`'s KEM key, once it has a repo here. */
	kemKey(did: string): string | null {
		return this.repos.get(did)?.kem.did() ?? null;
	}

	/** Replaces `did`'s KEM key, as a PDS rotating it does, while cached DID documents still have the old one. */
	async rotateKemKey(did: string): Promise<void> {
		const repo = await this.repo(did);
		this.identity.staleKemKeys.set(did, repo.kem.did());
		repo.kem = await P256Keypair.create();
	}

	/** Creates or updates a record in `did`'s repo in the space. */
	async put(did: string, collection: string, rkey: string, value: LexMap): Promise<void> {
		if (!isNsidString(collection) || !isSpaceKeyString(rkey)) throw new Error(`Bad record path ${collection}/${rkey}`);
		const repo = await this.repo(did);
		const { cid, bytes } = await serializeRecord(collection, rkey, value);
		const prev = repo.records.get(`${collection}/${rkey}`)?.cid.toString() ?? null;
		repo.records.set(`${collection}/${rkey}`, { collection, rkey, cid, bytes, value });
		this.write(repo, { collection, rkey, cid: cid.toString(), prev });
	}

	async delete(did: string, collection: string, rkey: string): Promise<void> {
		const repo = await this.repo(did);
		const record = repo.records.get(`${collection}/${rkey}`);

		if (!record) return;
		repo.records.delete(`${collection}/${rkey}`);
		this.write(repo, { collection: record.collection, rkey: record.rkey, cid: null, prev: record.cid.toString() });
	}

	readonly fetch = async (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
		const request = new Request(input, init);
		const url = new URL(request.url);
		const nsid = url.pathname.replace(/^\/xrpc\//, '');
		const body: unknown = request.method === 'POST' ? await request.json() : undefined;
		this.requests.push({ nsid, headers: request.headers, body });

		if (nsid === 'com.atproto.space.getSpaceCredential') {
			const asked = credentialRequest.safeParse(body);

			return this.issueCredential(asked.success ? asked.data : null);
		}

		const authorization = /^Atproto-Space-Credential (\S+)$/.exec(request.headers.get('authorization') ?? '');

		if (!authorization) return xrpcError(400, 'InvalidToken');

		if (this.revoked.has(authorization[1])) return xrpcError(401, 'CredentialRevoked');

		try {
			const key = parseSpaceToken('credential', authorization[1]).payload.cnf?.kid;

			if (!key) throw new Error('Credential is not bound to a key');
			await verifyCredentialSig(Object.fromEntries(request.headers), key);
		} catch {
			return xrpcError(400, 'BadSpaceSignature');
		}

		const params = Object.fromEntries(url.searchParams);

		if (nsid === 'com.atproto.space.listRepos') {
			const since = params.cursor ?? '';
			const repos = [...this.repos].filter(([, repo]) => repo.rev > since);

			return Response.json({ repos: repos.map(([did, { rev }]) => ({ did, repoRev: rev, spaceRev: rev })) });
		}

		const repo = this.repos.get(params.repo);

		if (!repo || !isDidString(params.repo)) return xrpcError(400, 'RepoNotFound');
		const want = request.headers.get(WANT_MAC_HEADER) ?? undefined;
		const type: MacType = nsid === 'com.atproto.space.getRecord' ? RECORD_MAC_TYPE : COMMIT_MAC_TYPE;
		let sharedSecret: Uint8Array;

		try {
			sharedSecret = this.forgeMacs
				? crypto.getRandomValues(new Uint8Array(32))
				: decapsulate(repo.kem, parseWantMacHeader(want, type));
		} catch {
			return xrpcError(400, 'BadSpaceMac');
		}

		const commit = RepoCommit.fromRecords(repo.records.values()).toCommit({
			space: this.space,
			repo: params.repo,
			rev: repo.rev
		});

		const commitMac = { [MAC_HEADER]: formatMacHeader(COMMIT_MAC_TYPE, computeCommitMac(sharedSecret, commit)) };

		if (nsid === 'com.atproto.space.listRepoOps') {
			const from = params.cursor ? Number(params.cursor) : repo.ops.filter((op) => op.rev <= params.since).length;

			const next = from + Math.min(Number(params.limit), this.pageSize);

			const ops = repo.ops.slice(from, next).map((op) => {
				const current = repo.records.get(`${op.collection}/${op.rkey}`);

				return current?.cid.toString() === op.cid ? { ...op, value: current.value } : op;
			});

			if (next < repo.ops.length) return new Response(lexStringify({ ops, cursor: String(next) }));

			return new Response(lexStringify({ ops, commit }), { headers: commitMac });
		}

		if (nsid === 'com.atproto.space.getRepo') {
			const chunks: Uint8Array[] = [];
			// A STAR archive lists records in path order.
			const records = [...repo.records].sort(([a], [b]) => (a < b ? -1 : 1)).map(([, record]) => record);

			for await (const chunk of serializeRepo(commit, records)) chunks.push(chunk);

			return new Response(Buffer.concat(chunks), {
				headers: { 'content-type': 'application/octet-stream', ...commitMac }
			});
		}

		const record = repo.records.get(`${params.collection}/${params.rkey}`);

		if (!record) return xrpcError(400, 'RecordNotFound');
		const uri = spaceRecordUri(this.space, params.repo, record.collection, record.rkey);
		const mac = formatMacHeader(RECORD_MAC_TYPE, computeRecordMac(sharedSecret, uri, record.cid));

		return new Response(lexStringify({ uri, cid: record.cid.toString(), value: record.value }), {
			headers: { [MAC_HEADER]: mac }
		});
	};

	private async issueCredential(input: z.infer<typeof credentialRequest> | null): Promise<Response> {
		if (this.credentialError) return xrpcError(400, this.credentialError);

		if (!input || input.space !== this.space || !isP256DidKey(input.credentialKid)) {
			return xrpcError(400, 'InvalidRequest');
		}

		const credential = await createSpaceToken(
			'credential',
			{ iss: parseSpaceUri(this.space)!.authority, sub: this.space, keyId: input.credentialKid },
			this.authorityKey
		);

		this.issued.push(credential);

		return Response.json({ credential });
	}

	private async repo(did: string): Promise<HostedRepo> {
		const existing = this.repos.get(did);

		if (existing) return existing;
		const created: HostedRepo = { kem: await P256Keypair.create(), rev: '', records: new Map(), ops: [] };
		this.repos.set(did, created);

		return created;
	}

	private write(repo: HostedRepo, op: HostedOp): void {
		repo.rev = TID.nextStr();
		repo.ops.push({ ...op, rev: repo.rev });
	}
}
