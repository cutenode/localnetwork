import { TID } from '@atproto/common-web';
import { P256Keypair } from '@atproto/crypto';
import { encode } from '@atproto/lex-cbor';
import { type Cid, cidForCbor, type LexMap } from '@atproto/lex-data';
import { lexStringify } from '@atproto/lex-json';
import { isDidString, isNsidString, isSpaceKeyString } from '@atproto/oauth-scopes';
import { isSpaceAtUriString } from '@atproto/syntax';
import {
	COMMIT_MAC_PROFILE,
	createCommitMacHeaders,
	createRecordMacHeaders,
	createSpaceCredential,
	decapsulate,
	isP256DidKey,
	type MacProfile,
	parseMacRequest,
	parseSpaceToken,
	RECORD_MAC_PROFILE,
	type RecordPathParts,
	RepoCommit,
	type SpaceCommit,
	verifyCredentialSig,
	writeRepoStar
} from '@atproto/space';
import { z } from 'zod';
import { IdentityResolver, type ResolvedIdentity } from '../atproto/identity';
import { parseSpaceUri, spaceRecordUri } from '../atproto/spaces';
import { silentLogger } from '../logger';

/** A record, and its value unless it was put as bytes only, which listRepoOps then serves without one. */
type HostedRecord = RecordPathParts & { cid: Cid; bytes: Uint8Array; value?: LexMap };

type HostedOp = RecordPathParts & { cid: string | null; prev: string | null };

type HostedRepo = {
	kem: P256Keypair;
	rev: string;
	/** By `collection/rkey`. */
	records: Map<string, HostedRecord>;
	ops: (HostedOp & { rev: string })[];
};

const authorizeRequest = z.object({
	space: z.string(),
	credentialKey: z.string(),
	delegationToken: z.string(),
	notify: z.object({ endpoint: z.string(), token: z.string() }).optional()
});

const xrpcError = (status: number, error: string) => Response.json({ error }, { status });

const isSpaceRef = (value: string): value is SpaceCommit['space'] => parseSpaceUri(value) !== null;

/**
 * Resolves every account to the host and the KEM key it holds for them, as a DID document cache would, and sends
 * every request for a service to the host.
 */
class HostIdentity extends IdentityResolver {
	/** KEM keys cached from before a rotation, served until a forced refresh. */
	readonly staleKemKeys = new Map<string, string>();
	/** Accounts whose DID documents publish no KEM key. */
	readonly withoutKemKeys = new Set<string>();
	/** DIDs resolved with a forced refresh, in order. */
	readonly refreshed: string[] = [];

	constructor(private readonly host: FakeSpaceHost) {
		super({
			plcUrl: 'https://plc.test',
			allowPrivateNetwork: true,
			logger: silentLogger,
			// The host's own fetch is a field set after this one.
			fetch: (input, init) => host.fetch(input, init)
		});
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
 * the alpha PDS does: credentials are asked for with authorizeSpaceSync's JSON body, every other request has to
 * carry one in a signed `Atproto-Space-Credential` header, and listRepoOps, getRepo, and getRecord answer with a
 * MAC keyed to the author's KEM key.
 */
export class FakeSpaceHost {
	readonly url = 'https://host.test';
	/** Every request it got, in order. */
	readonly requests: { nsid: string; headers: Headers; body: unknown }[] = [];
	/** Ops per listRepoOps page, whatever the limit asked for. */
	pageSize = 500;
	/** An error authorizeSpaceSync answers with, as an authority does for a deleted space or a bad token. */
	credentialError: string | null = null;
	/** Whether the endpoint a credential asks to register confirms it, which it does unless it can't be reached. */
	confirmsRegistration = true;
	/** The registrations for updates it holds, by endpoint and token, and when each lapses. */
	readonly registrations: { endpoint: string; token: string; expiresAt: number }[] = [];
	/** Repos it took out of the space's writer set, which listWriters reports once their removal comes up. */
	readonly removed = new Map<string, string>();
	/** Credentials it has revoked, which it answers CredentialRevoked to. */
	readonly revoked = new Set<string>();
	/** Every credential it issued, in order. */
	readonly issued: string[] = [];
	/** An error the repo host answers its writers' repo reads with, as a hostile or broken PDS could. */
	repoError: string | null = null;
	/** Errors one writer's repo reads answer with, by DID, for one bad repo among good ones. */
	readonly repoErrors = new Map<string, string>();
	/** How many repos a listWriters page holds. */
	repoPageSize = 500;
	/** MACs responses with a secret of its own instead of the one it shares with the syncer. */
	forgeMacs = false;
	/** Answers a method gets in place of the real one, by NSID, as a hostile host could send. */
	readonly overrides = new Map<string, () => Response>();
	readonly identity = new HostIdentity(this);
	private readonly repos = new Map<string, HostedRepo>();

	private constructor(
		readonly space: SpaceCommit['space'],
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
		await this.putEncoded(did, collection, rkey, encode(value), value);
	}

	/**
	 * Creates or updates a record from its DRISL bytes, as a writer can store bytes the encoder here won't make, such
	 * as a record nested deep enough to overflow its stack.
	 */
	async putEncoded(did: string, collection: string, rkey: string, bytes: Uint8Array, value?: LexMap): Promise<void> {
		if (!isNsidString(collection) || !isSpaceKeyString(rkey)) throw new Error(`Bad record path ${collection}/${rkey}`);
		const repo = await this.repo(did);
		const cid = await cidForCbor(bytes);
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

	/** Takes `did` out of the space's writer set, as removing a member does. */
	removeWriter(did: string): void {
		this.removed.set(did, TID.nextStr());
	}

	readonly fetch = async (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
		const request = new Request(input, init);
		const url = new URL(request.url);
		const nsid = url.pathname.replace(/^\/xrpc\//, '');
		const body: unknown = request.method === 'POST' ? await request.json() : undefined;
		this.requests.push({ nsid, headers: request.headers, body });
		const override = this.overrides.get(nsid);

		if (override) return override();

		if (nsid === 'com.atproto.space.authorizeSpaceSync') {
			const asked = authorizeRequest.safeParse(body);

			return this.issueCredential(asked.success ? asked.data : null);
		}

		const authorization = /^Atproto-Space-Credential (\S+)$/.exec(request.headers.get('authorization') ?? '');

		if (!authorization) return xrpcError(400, 'InvalidToken');

		if (this.revoked.has(authorization[1])) return xrpcError(401, 'CredentialRevoked');

		try {
			const key = parseSpaceToken('credential', authorization[1]).payload.cnf.kid;
			await verifyCredentialSig(Object.fromEntries(request.headers), key);
		} catch {
			return xrpcError(400, 'BadSpaceSignature');
		}

		const params = Object.fromEntries(url.searchParams);

		if (nsid === 'com.atproto.space.listWriters') {
			const since = params.cursor ?? '';

			// A repo's space revision is its own last one, or its removal's.
			const writers = [
				...[...this.repos].flatMap(([repo, { rev }]) =>
					this.removed.has(repo)
						? []
						: [{ $type: 'com.atproto.space.defs#repoWrite', repo, repoRev: rev, data: { $bytes: '' }, spaceRev: rev }]
				),
				...[...this.removed].map(([repo, spaceRev]) => ({
					$type: 'com.atproto.space.defs#repoRemoved',
					repo,
					spaceRev
				}))
			]
				.filter((writer) => writer.spaceRev > since)
				.toSorted((a, b) => (a.spaceRev < b.spaceRev ? -1 : 1));

			const page = writers.slice(0, this.repoPageSize);
			const cursor = writers.length > page.length ? page.at(-1)?.spaceRev : undefined;

			return Response.json({ repos: page, cursor });
		}

		const repoError = this.repoError ?? this.repoErrors.get(params.repo);

		if (repoError) return xrpcError(400, repoError);
		const repo = this.repos.get(params.repo);

		if (!repo || !isDidString(params.repo)) return xrpcError(400, 'RepoNotFound');
		const profile: MacProfile = nsid === 'com.atproto.space.getRecord' ? RECORD_MAC_PROFILE : COMMIT_MAC_PROFILE;
		const asked = parseMacRequest(request.headers.get('accept-signature') ?? undefined, profile);

		if (!asked) return xrpcError(400, 'InvalidSpaceMac');

		const sharedSecret = this.forgeMacs
			? crypto.getRandomValues(new Uint8Array(32))
			: decapsulate(repo.kem, asked.keyShare);

		const mac = { ...asked, sharedSecret };

		const commit = RepoCommit.fromRecords(repo.records.values()).toCommit({
			space: this.space,
			repo: params.repo,
			rev: repo.rev
		});

		const { headers: commitMac } = createCommitMacHeaders(commit, mac);

		if (nsid === 'com.atproto.space.listRepoOps') {
			const from = params.cursor ? Number(params.cursor) : repo.ops.filter((op) => op.rev <= params.since).length;

			const next = from + Math.min(Number(params.limit), this.pageSize);

			const ops = repo.ops.slice(from, next).map((op) => {
				const current = repo.records.get(`${op.collection}/${op.rkey}`);

				return current?.value && current.cid.toString() === op.cid ? { ...op, value: current.value } : op;
			});

			const cursor = next < repo.ops.length ? String(next) : undefined;

			return new Response(lexStringify({ ops, commit, cursor }), { headers: commitMac });
		}

		if (nsid === 'com.atproto.space.getRepo') {
			const chunks: Uint8Array[] = [];
			// A STAR archive lists records in path order.
			const records = [...repo.records].sort(([a], [b]) => (a < b ? -1 : 1)).map(([, record]) => record);

			for await (const chunk of writeRepoStar(commit, records)) chunks.push(chunk);

			return new Response(Buffer.concat(chunks), {
				headers: { 'content-type': 'application/vnd.dasl.star', ...commitMac }
			});
		}

		const record = repo.records.get(`${params.collection}/${params.rkey}`);

		if (!record) return xrpcError(400, 'RecordNotFound');
		const uri = spaceRecordUri(this.space, params.repo, record.collection, record.rkey);

		if (!isSpaceAtUriString(uri)) return xrpcError(400, 'InvalidRequest');
		const { headers } = createRecordMacHeaders(uri, record.cid, mac);

		return new Response(lexStringify({ uri, cid: record.cid.toString(), value: record.value }), { headers });
	};

	private async issueCredential(input: z.infer<typeof authorizeRequest> | null): Promise<Response> {
		if (this.credentialError) return xrpcError(400, this.credentialError);
		const authority = parseSpaceUri(this.space)?.authority;

		if (!input || input.space !== this.space || !isP256DidKey(input.credentialKey) || !isDidString(authority)) {
			return xrpcError(400, 'InvalidRequest');
		}

		if (input.notify && !this.confirmsRegistration) return xrpcError(400, 'RegistrationNotConfirmed');

		const credential = await createSpaceCredential({
			iss: authority,
			sub: this.space,
			keyId: input.credentialKey,
			keypair: this.authorityKey
		});

		this.issued.push(credential);

		if (input.notify) {
			const expiresAt = parseSpaceToken('credential', credential).payload.exp * 1000;
			this.registrations.push({ ...input.notify, expiresAt });
		}

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
