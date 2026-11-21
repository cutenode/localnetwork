import { fetchResponseMaxSizeChecker } from '@atproto-labs/fetch';
import { type Cid, isLexMap, isObject, type LexMap, parseCid } from '@atproto/lex-data';
import { type JsonObject, type JsonValue, jsonToLex, lexToJson } from '@atproto/lex-json';
import { isDidString, isNsidString, isSpaceKeyString } from '@atproto/oauth-scopes';
import { COMMIT_MAC_PROFILE, type RecordPathParts, RepoCommit, type SpaceCommit, verifyCommit } from '@atproto/space';
import { isSpaceRefString } from '@atproto/syntax';
import { timingSafeEqual } from 'node:crypto';
import PQueue from 'p-queue';
import { z } from 'zod';
import {
	type NotifyRegistration,
	type SpaceCredentialManager,
	isCredentialRejected,
	isMacFailure,
	readAnswer,
	XrpcResponseError
} from '../atproto/credentials';
import type { IdentityResolver } from '../atproto/identity';
import { newToken } from '../auth/session';
import { isTooDeep, TOO_DEEP } from '../atproto/lexicons';
import { parseSpaceUri, spaceRecordUri } from '../atproto/spaces';
import { type Db, nowIso, parseJson } from '../db';
import type { Logger } from '../logger';
import { isSpaceCommit, readRepoArchive } from './repo-archive';

export type SyncedChange =
	| {
			kind: 'upsert';
			space: string;
			repoDid: string;
			collection: string;
			rkey: string;
			uri: string;
			cid: string;
			value: JsonObject;
	  }
	| { kind: 'delete'; space: string; repoDid: string; collection: string; rkey: string; uri: string };

export type ChangeHandler = (change: SyncedChange) => Promise<void>;

const REPO_WRITE = 'com.atproto.space.defs#repoWrite';

/**
 * A listWriters page. Besides each repo's latest write, it lists repos removed from the writer set, which the syncer
 * only reads past: their records stay as they were synced, as they did before hosts reported removals. The union is
 * open, so an update of a kind this app doesn't know is read past too.
 */
const listWritersOutput = z.object({
	repos: z.array(
		z.object({ $type: z.string(), repo: z.string(), repoRev: z.string().optional(), spaceRev: z.string() })
	),
	cursor: z.string().optional()
});

type RepoEntry = { repo: string; repoRev: string };

const listRepoOpsOutput = z.object({
	ops: z.array(
		z.object({
			collection: z.custom<RecordPathParts['collection']>(isNsidString),
			// Record keys and space keys share one syntax.
			rkey: z.custom<RecordPathParts['rkey']>(isSpaceKeyString),
			cid: z.string().nullable(),
			prev: z.string().nullable(),
			value: z.custom<LexMap>(isLexMap).optional()
		})
	),
	commit: z.custom<SpaceCommit>(isSpaceCommit),
	cursor: z.string().optional()
});

export type SyncStatus = {
	space: string;
	type: string;
	skey: string;
	spaceRev: string | null;
	lastSyncedAt: string | null;
	lastError: string | null;
	registrationExpiresAt: string | null;
	/** A repo that has failed since before it ever synced has no rev or sync time. */
	repos: { did: string; rev: string | null; lastSyncedAt: string | null; lastError: string | null }[];
	/** Records whose projection has failed at least once and is waiting for its next try. */
	stuck: {
		did: string;
		collection: string;
		rkey: string;
		attempts: number;
		nextAttemptAt: string;
		lastError: string | null;
	}[];
};

/**
 * A listRepoOps page read from its text, with the indexes of the ops whose values are nested deeper than
 * validateRecord allows. jsonToLex recurses, so those values are taken out before it decodes the rest. A page that
 * doesn't read (not JSON, the wrong shape, or nested too deep anywhere else) is the host's failure, so it rejects as
 * one, and syncRepo downloads the repo in full instead. @internal
 */
export function readOpsPage(text: string) {
	const nsid = 'com.atproto.space.listRepoOps';
	const json: JsonValue = readAnswer(nsid, () => JSON.parse(text));
	const tooDeep = new Set<number>();
	const ops = isObject(json) && !Array.isArray(json) && Array.isArray(json.ops) ? json.ops : [];

	for (const [index, op] of ops.entries()) {
		if (!isObject(op) || Array.isArray(op) || !isTooDeep(op.value)) continue;
		tooDeep.add(index);
		delete op.value;
	}

	return { page: readAnswer(nsid, () => listRepoOpsOutput.parse(jsonToLex(json))), tooDeep };
}

// SAFETY: lexToJson only rewrites the CIDs and bytes inside a map, so a LexMap always comes back as a JSON object.
const toJson = (record: LexMap) => lexToJson(record) as JsonObject;

const asCid = (value: string | null): Cid | null => (value ? parseCid(value) : null);

/** The space and repo a commit has to name, typed as @atproto/space checks them. */
function commitScope(space: string, repo: string) {
	if (!isSpaceRefString(space) || !isDidString(repo)) throw new Error(`Not a space and a repo DID: ${space} ${repo}`);

	return { space, repo };
}

/**
 * Whether the space's authority said the space is gone: authorizeSpaceSync and listWriters only go to the
 * authority. A writer's repo host answering SpaceDeleted says nothing about the space, and neither does a
 * credential renewal that fails any other way, SpaceNotFound included, which hosts may answer in place of a refusal.
 */
function authoritySaysGone(error: XrpcResponseError): boolean {
	if (error.nsid === 'com.atproto.space.authorizeSpaceSync') return error.error === 'SpaceDeleted';

	return (
		error.nsid === 'com.atproto.space.listWriters' &&
		(error.error === 'SpaceDeleted' || error.error === 'SpaceNotFound')
	);
}

/** Getting a credential fails for every repo in the space, so it ends the pass instead of counting against one repo. */
const failsWholeSpace = (cause: unknown) =>
	cause instanceof XrpcResponseError && cause.nsid === 'com.atproto.space.authorizeSpaceSync';

const REPO_UNAVAILABLE = new Set(['RepoTakendown', 'RepoSuspended', 'RepoDeactivated']);

/**
 * Whether a writer's repo host won't serve the repo because the account is taken down, suspended, or
 * deactivated. A full download fails the same way, and the pass carries on without it: the repo waits out its
 * backoff, so it's picked up again once the account is back. The same errors from authorizeSpaceSync mean the
 * authority's account is, which suspends the whole space.
 */
const isRepoUnavailable = (cause: unknown) =>
	cause instanceof XrpcResponseError && REPO_UNAVAILABLE.has(cause.error) && !failsWholeSpace(cause);

/**
 * The most pages of a listWriters or listRepoOps listing one pass reads. The host decides when its listing ends, so
 * without a limit one that keeps answering with a cursor would hold the space's pass, and every pass after it.
 */
const MAX_PAGES = 100;

/**
 * The most of a repo export (`getRepo`) one download reads, in bytes. Every record in it is held in memory until the
 * download is applied, and a hostile repo host could send one that never ends. A record is at most 1 MiB
 * (@atproto/space's MAX_RECORD_SIZE), but the records this app writes are a few KiB at most. The largest repo is the
 * root's in the audit space, which gains an entry for every audited action, most well under 1 KiB, so this holds a
 * few hundred thousand of them, many times what any repo here is expected to hold. A repo past it fails and waits
 * for its retry like any other, and the system page shows it.
 */
const REPO_EXPORT_MAX_BYTES = 256 * 1024 * 1024;

/** What the response cap @atproto-labs/fetch applies (serviceApiFetch's too) fails a body past it with. */
const TOO_LARGE = 'Response too large';

/** How long a repo waits after its `attempts`th failure in a row: 1 minute, doubling up to an hour. */
const retryDelayMs = (attempts: number) => Math.min(60_000 * 2 ** (attempts - 1), 3_600_000);

/** The retry columns after one more failure. */
const nextRetry = (attempts: number, message: string) => ({
	attempts: attempts + 1,
	nextAttemptAt: new Date(Date.now() + retryDelayMs(attempts + 1)).toISOString(),
	lastError: message.slice(0, 500)
});

/**
 * Keeps a local copy of every watched space: enumerates writers with
 * `listWriters` on the space host, pulls each repo's oplog with
 * `listRepoOps`, checks the result against the LtHash commit (authenticated
 * by a MAC keyed to the author's `#atproto_kem` key), and falls back to a
 * full verified STAR download (`getRepo`) when the incremental path cannot be
 * trusted. A repo that fails doesn't hold back the rest: the pass carries on,
 * its cursor moves past the repo, and `syncRepoRetry` keeps it for a retry,
 * since `listWriters` won't list it again until it next changes. A record is
 * stored and queued for projection in one transaction, so one whose
 * projection throws, or never runs, stays in `pendingProjection` for later
 * passes once the repo's cursor has moved on.
 */
export class SpaceSyncer {
	private readonly spaceLocks = new Map<string, Promise<void>>();
	private readonly pending = new Set<string>();
	private readonly repoQueue = new PQueue({ concurrency: 4 });

	constructor(
		private readonly deps: {
			db: Db;
			logger: Logger;
			identity: IdentityResolver;
			credentials: SpaceCredentialManager;
			onChange: ChangeHandler;
			/** Forgets what was derived from a space as a whole, not from its records, once the space is dropped. */
			onSpaceDropped: (space: string) => Promise<void>;
			/** The base URL a space host sends this app its spaces' updates at, or null to rely on polling. */
			notifyEndpoint: string | null;
			/** The most pages of a listing one pass reads, MAX_PAGES unless a test needs a host's endless listing short. */
			maxPages?: number;
			/** The most bytes of a repo export one download reads, REPO_EXPORT_MAX_BYTES unless a test needs it small. */
			maxRepoBytes?: number;
		}
	) {}

	async watch(space: string): Promise<void> {
		const ref = parseSpaceUri(space);

		if (!ref) throw new Error(`Not a space URI: ${space}`);
		await this.deps.db
			.insertInto('spaceWatch')
			.values({
				spaceUri: space,
				spaceType: ref.type,
				authorityDid: ref.authority,
				skey: ref.skey,
				spaceRev: null,
				registrationExpiresAt: null,
				notifyToken: newToken(),
				lastSyncedAt: null,
				lastError: null,
				createdAt: nowIso()
			})
			.onConflict((oc) => oc.doNothing())
			.execute();
	}

	async unwatch(space: string): Promise<void> {
		await this.deps.db.deleteFrom('spaceWatch').where('spaceUri', '=', space).execute();
	}

	/** What a credential for a watched space registers, so its host sends the space's updates here. */
	async registration(space: string): Promise<NotifyRegistration | null> {
		const endpoint = this.deps.notifyEndpoint;
		const token = endpoint ? await this.notifyToken(space) : undefined;

		return endpoint && token ? { endpoint, token } : null;
	}

	/** Whether `token` is the one this app registered for `space`, which is how a host's notice says it's from the host. */
	async isRegistered(space: string, token: string): Promise<boolean> {
		const expected = Buffer.from((await this.notifyToken(space)) ?? '');
		const given = Buffer.from(token);

		return expected.length > 0 && given.length === expected.length && timingSafeEqual(given, expected);
	}

	private async notifyToken(space: string): Promise<string | undefined> {
		const watch = await this.deps.db
			.selectFrom('spaceWatch')
			.select('notifyToken')
			.where('spaceUri', '=', space)
			.executeTakeFirst();

		return watch?.notifyToken;
	}

	/** Syncs every watched space; errors are recorded per space, not thrown. */
	async syncAll(): Promise<void> {
		const watches = await this.deps.db.selectFrom('spaceWatch').select('spaceUri').execute();
		await Promise.all(watches.map(({ spaceUri }) => this.sync(spaceUri).catch(() => undefined)));
	}

	/**
	 * Syncs one space. Concurrent calls for the same space coalesce: at most
	 * one pass runs and one more is queued behind it.
	 */
	sync(space: string): Promise<void> {
		const running = this.spaceLocks.get(space);

		if (running) {
			if (this.pending.has(space)) return running;
			this.pending.add(space);

			const next = this.release(
				space,
				running
					.catch(() => undefined)
					.then(() => {
						this.pending.delete(space);

						return this.syncSpace(space);
					})
			);

			this.spaceLocks.set(space, next);

			return next;
		}

		const task = this.release(space, this.syncSpace(space));
		this.spaceLocks.set(space, task);

		return task;
	}

	/** Frees the space's lock once `pass` ends, unless a queued pass has taken it over. */
	private release(space: string, pass: Promise<void>): Promise<void> {
		const task: Promise<void> = pass.finally(() => {
			if (this.spaceLocks.get(space) === task) this.spaceLocks.delete(space);
		});

		return task;
	}

	private syncSpace(space: string): Promise<void> {
		return this.deps.logger.job('space-sync', () => this.pass(space), { space });
	}

	private async pass(space: string): Promise<void> {
		const { db, logger } = this.deps;
		const watch = await db.selectFrom('spaceWatch').selectAll().where('spaceUri', '=', space).executeTakeFirst();

		if (!watch) return;

		try {
			// An authority's PDS is its space host.
			const authority = await this.deps.identity.resolve(watch.authorityDid);
			let cursor = watch.spaceRev ?? undefined;
			let changed = 0;
			const failures: unknown[] = [];

			const settle = async (syncs: Promise<boolean>[]) => {
				const results = await Promise.allSettled(syncs);

				for (const result of results) {
					if (result.status === 'fulfilled') changed += Number(result.value);
					else if (failsWholeSpace(result.reason)) throw result.reason;
					else if (!isRepoUnavailable(result.reason)) failures.push(result.reason);
				}
			};

			for (let pages = 1; ; pages++) {
				const page = await this.deps.credentials.query({
					space,
					service: authority.pds,
					audience: watch.authorityDid,
					nsid: 'com.atproto.space.listWriters',
					params: { space, limit: 500, cursor },
					schema: listWritersOutput
				});

				const last = page.repos.at(-1);

				if (!last) break;

				const writes = page.repos.flatMap(({ $type, repo, repoRev }) =>
					$type === REPO_WRITE && repoRev ? [{ repo, repoRev }] : []
				);

				await settle(writes.map((repo) => this.repoQueue.add(() => this.syncRepoIfBehind(space, repo))));
				cursor = page.cursor ?? last.spaceRev;

				if (!page.cursor) break;

				// The cursor is kept, so the next pass reads on from here.
				if (pages === (this.deps.maxPages ?? MAX_PAGES)) {
					logger.warn('repo list ran past the page limit; reading on next pass', { pages });
					break;
				}
			}

			const due = await db
				.selectFrom('syncRepoRetry')
				.select('repoDid')
				.where('spaceUri', '=', space)
				.where('nextAttemptAt', '<=', nowIso())
				.execute();

			await settle(
				due.map(({ repoDid }) =>
					this.repoQueue.add(async () => {
						await this.syncRepo(space, repoDid);

						return true;
					})
				)
			);

			await this.retryProjections(space);

			// A repo or record waiting out its backoff still counts as failing, so the system page keeps showing it.
			const waiting = await db
				.selectFrom('syncRepoRetry')
				.select('lastError')
				.where('spaceUri', '=', space)
				.unionAll(
					db
						.selectFrom('pendingProjection')
						.select('lastError')
						.where('spaceUri', '=', space)
						.where('lastError', 'is not', null)
						.$narrowType<{ lastError: string }>()
				)
				.executeTakeFirst();

			await db
				.updateTable('spaceWatch')
				.set({
					spaceRev: cursor ?? null,
					// Each credential minted for the space renews its registration for updates.
					registrationExpiresAt: this.deps.credentials.registeredUntil(space),
					lastSyncedAt: nowIso(),
					lastError: waiting?.lastError ?? null
				})
				.where('spaceUri', '=', space)
				.execute();

			if (changed > 0) logger.debug('space synced', { changed });

			if (failures.length > 0) throw failures[0];
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			await db
				.updateTable('spaceWatch')
				.set({ lastError: message.slice(0, 500) })
				.where('spaceUri', '=', space)
				.execute();

			if (error instanceof XrpcResponseError && authoritySaysGone(error)) {
				logger.warn('space is gone; dropping local copy');
				await this.dropSpace(space);

				return;
			}

			if (watch.lastError === message.slice(0, 500)) logger.debug('space sync still failing', { error: message });
			else logger.warn('space sync failed', { err: error });
			throw error;
		}
	}

	private async syncRepoIfBehind(space: string, repo: RepoEntry): Promise<boolean> {
		const local = await this.deps.db
			.selectFrom('syncedRepo')
			.select(['rev'])
			.where('spaceUri', '=', space)
			.where('repoDid', '=', repo.repo)
			.executeTakeFirst();

		if (local && local.rev >= repo.repoRev) return false;

		const retry = await this.deps.db
			.selectFrom('syncRepoRetry')
			.select('nextAttemptAt')
			.where('spaceUri', '=', space)
			.where('repoDid', '=', repo.repo)
			.executeTakeFirst();

		if (retry && retry.nextAttemptAt > nowIso()) return false;
		await this.syncRepo(space, repo.repo);

		return true;
	}

	/** Brings one repo in a space up to date, verifying it against the author's MAC-authenticated commit. */
	async syncRepo(space: string, repoDid: string): Promise<void> {
		const { db, logger } = this.deps;

		const local = await db
			.selectFrom('syncedRepo')
			.selectAll()
			.where('spaceUri', '=', space)
			.where('repoDid', '=', repoDid)
			.executeTakeFirst();

		try {
			await this.deps.credentials.readVerified(repoDid, async ({ pds }, kemKey) => {
				if (local) {
					try {
						await this.incremental(space, repoDid, pds, kemKey, local.rev, local.ltHash);

						return;
					} catch (error) {
						// A full download would fail the same way: readVerified retries a MAC failure with a fresh KEM key.
						if (isCredentialRejected(error) || isMacFailure(error) || isRepoUnavailable(error)) throw error;
						logger.info('incremental sync failed; recovering full repo', { err: error, space, repoDid });
					}
				}

				await this.recover(space, repoDid, pds, kemKey);
			});

			await db.deleteFrom('syncRepoRetry').where('spaceUri', '=', space).where('repoDid', '=', repoDid).execute();
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);

			if (!failsWholeSpace(error)) await this.recordFailure(space, repoDid, message);

			if (isRepoUnavailable(error))
				logger.info('skipping a repo its host will not serve', { err: error, space, repoDid });

			if (local) {
				await db
					.updateTable('syncedRepo')
					.set({ lastError: message.slice(0, 500) })
					.where('spaceUri', '=', space)
					.where('repoDid', '=', repoDid)
					.execute();
			}

			throw error;
		}
	}

	private async recordFailure(space: string, repoDid: string, message: string) {
		const { db } = this.deps;

		const previous = await db
			.selectFrom('syncRepoRetry')
			.select('attempts')
			.where('spaceUri', '=', space)
			.where('repoDid', '=', repoDid)
			.executeTakeFirst();

		const values = nextRetry(previous?.attempts ?? 0, message);

		await db
			.insertInto('syncRepoRetry')
			.values({ spaceUri: space, repoDid, ...values })
			.onConflict((oc) => oc.columns(['spaceUri', 'repoDid']).doUpdateSet(values))
			.execute();
	}

	private async incremental(
		space: string,
		repoDid: string,
		pds: string,
		kemKey: string,
		since: string,
		ltHash: Uint8Array
	) {
		const state = RepoCommit.fromState(ltHash);
		const changes: SyncedChange[] = [];
		let cursor: string | undefined;
		let commit: SpaceCommit;
		const maxPages = this.deps.maxPages ?? MAX_PAGES;
		let pages = 0;

		do {
			// A full download replaces an oplog this long in one request, so syncRepo falls back to one.
			if (++pages > maxPages) throw new Error(`Oplog ran past ${maxPages} pages`);

			const { response, mac } = await this.deps.credentials.macRequest({
				space,
				service: pds,
				audience: repoDid,
				nsid: 'com.atproto.space.listRepoOps',
				params: { space, repo: repoDid, since: cursor ? undefined : since, cursor, limit: 500 },
				mac: { profile: COMMIT_MAC_PROFILE, kemKey }
			});

			const { page, tooDeep } = readOpsPage(await response.text());

			for (const [index, op] of page.ops.entries()) {
				state.applyOp({ collection: op.collection, rkey: op.rkey, cid: asCid(op.cid), prev: asCid(op.prev) });
				const uri = spaceRecordUri(space, repoDid, op.collection, op.rkey);

				// A record too deep to read is dropped from the local copy, like one deleted.
				if (tooDeep.has(index)) this.deps.logger.warn(`dropping a synced record ${TOO_DEEP}`, { uri });

				if (op.cid === null || tooDeep.has(index)) {
					changes.push({ kind: 'delete', space, repoDid, collection: op.collection, rkey: op.rkey, uri });
				} else if (op.value !== undefined) {
					changes.push({
						kind: 'upsert',
						space,
						repoDid,
						collection: op.collection,
						rkey: op.rkey,
						uri,
						cid: op.cid,
						value: toJson(op.value)
					});
				} else {
					// Superseded intermediate value: a later op in this batch carries the current one.
					changes.push({
						kind: 'upsert',
						space,
						repoDid,
						collection: op.collection,
						rkey: op.rkey,
						uri,
						cid: op.cid,
						value: {}
					});
				}
			}

			// Every page carries the repo's current commit with its MAC. The last page's is the one the oplog adds up to.
			verifyCommit(page.commit, { ...commitScope(space, repoDid), ...mac });
			commit = page.commit;
			cursor = page.cursor;
		} while (cursor);

		if (!state.matches(commit)) throw new Error('Set hash does not match the commit');

		const latest = this.latestPerPath(changes);

		for (const change of latest) {
			if (change.kind === 'upsert' && Object.keys(change.value).length === 0) {
				throw new Error(`Oplog omitted the current value for ${change.collection}/${change.rkey}`);
			}
		}

		await this.apply(space, repoDid, pds, commit, state, latest);
	}

	private latestPerPath(changes: SyncedChange[]): SyncedChange[] {
		const byPath = new Map<string, SyncedChange>();

		for (const change of changes) byPath.set(`${change.collection}/${change.rkey}`, change);

		return [...byPath.values()];
	}

	private async recover(space: string, repoDid: string, pds: string, kemKey: string) {
		const { logger } = this.deps;
		const maxBytes = this.deps.maxRepoBytes ?? REPO_EXPORT_MAX_BYTES;

		const { response, mac } = await this.deps.credentials.macRequest({
			space,
			service: pds,
			audience: repoDid,
			nsid: 'com.atproto.space.getRepo',
			params: { space, repo: repoDid },
			mac: { profile: COMMIT_MAC_PROFILE, kemKey }
		});

		let archive: Awaited<ReturnType<typeof readRepoArchive>>;

		try {
			// Capped the way serviceApiFetch caps an answer read whole, at what a repo export can reach.
			const { body } = fetchResponseMaxSizeChecker(response, maxBytes);

			if (!body) throw new Error('Empty repo response');

			// A STAR archive: the commit, checked against its MAC, then every record, which must fold into its data.
			archive = await readRepoArchive(body, { ...commitScope(space, repoDid), ...mac });
		} catch (error) {
			if (error instanceof Error && error.message === TOO_LARGE) {
				logger.warn('repo export ran past the size cap; giving up on it', { space, repoDid, maxBytes });
			}

			throw error;
		}

		const existing = await this.deps.db
			.selectFrom('spaceRecord')
			.select(['collection', 'rkey', 'cid'])
			.where('spaceUri', '=', space)
			.where('repoDid', '=', repoDid)
			.execute();

		const present = new Set<string>();
		const changes: SyncedChange[] = [];
		const known = new Map(existing.map((row) => [`${row.collection}/${row.rkey}`, row.cid]));

		for (const { collection, rkey, cid, value } of archive.records) {
			const uri = spaceRecordUri(space, repoDid, collection, rkey);

			// A record too deep to read is dropped from the local copy, as the oplog drops one.
			if (value === null) {
				logger.warn(`dropping a synced record ${TOO_DEEP}`, { uri });
				continue;
			}

			const path = `${collection}/${rkey}`;
			present.add(path);

			if (known.get(path) === cid.toString()) continue;
			changes.push({ kind: 'upsert', space, repoDid, collection, rkey, uri, cid: cid.toString(), value });
		}

		for (const row of existing) {
			if (!present.has(`${row.collection}/${row.rkey}`)) {
				changes.push({
					kind: 'delete',
					space,
					repoDid,
					collection: row.collection,
					rkey: row.rkey,
					uri: spaceRecordUri(space, repoDid, row.collection, row.rkey)
				});
			}
		}

		await this.apply(space, repoDid, pds, archive.commit, archive.state, changes);
	}

	private async apply(
		space: string,
		repoDid: string,
		pds: string,
		commit: SpaceCommit,
		state: RepoCommit,
		changes: SyncedChange[]
	) {
		const { db } = this.deps;
		const now = nowIso();
		await db.transaction().execute(async (trx) => {
			for (const change of changes) {
				const pending = { attempts: 0, nextAttemptAt: now, lastError: null };
				await trx
					.insertInto('pendingProjection')
					.values({
						spaceUri: space,
						repoDid,
						collection: change.collection,
						rkey: change.rkey,
						...pending
					})
					.onConflict((oc) => oc.columns(['spaceUri', 'repoDid', 'collection', 'rkey']).doUpdateSet(pending))
					.execute();

				if (change.kind === 'delete') {
					await trx
						.deleteFrom('spaceRecord')
						.where('spaceUri', '=', space)
						.where('repoDid', '=', repoDid)
						.where('collection', '=', change.collection)
						.where('rkey', '=', change.rkey)
						.execute();
				} else {
					const value = JSON.stringify(change.value);
					await trx
						.insertInto('spaceRecord')
						.values({
							spaceUri: space,
							repoDid,
							collection: change.collection,
							rkey: change.rkey,
							cid: change.cid,
							value,
							indexedAt: now
						})
						.onConflict((oc) =>
							oc
								.columns(['spaceUri', 'repoDid', 'collection', 'rkey'])
								.doUpdateSet({ cid: change.cid, value, indexedAt: now })
						)
						.execute();
				}
			}

			const values = {
				pdsUrl: pds,
				rev: commit.rev,
				ltHash: state.setHash.state(),
				commitHash: commit.data,
				lastSyncedAt: now,
				lastError: null
			};

			await trx
				.insertInto('syncedRepo')
				.values({ spaceUri: space, repoDid, ...values })
				.onConflict((oc) => oc.columns(['spaceUri', 'repoDid']).doUpdateSet(values))
				.execute();
		});

		await this.project(changes.map((change) => ({ change, attempts: 0 })));
	}

	/** Runs each change's projection, which clears it from `pendingProjection` or leaves it there to try again. */
	private async project(pending: { change: SyncedChange; attempts: number }[]) {
		const { db, logger } = this.deps;

		for (const { change, attempts } of pending) {
			const { space: spaceUri, repoDid, collection, rkey } = change;
			const path = { spaceUri, repoDid, collection, rkey };

			try {
				await this.deps.onChange(change);
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error);
				await db
					.updateTable('pendingProjection')
					.set(nextRetry(attempts, message))
					.where((eb) => eb.and(path))
					.execute();

				if (attempts === 0) logger.warn('could not project synced record', { err: error, uri: change.uri });
				else logger.debug('synced record still failing to project', { error: message, uri: change.uri });
				continue;
			}

			await db
				.deleteFrom('pendingProjection')
				.where((eb) => eb.and(path))
				.execute();
		}
	}

	/** Projects what's due from earlier passes as the record stands now: gone from `spaceRecord` means deleted. */
	private async retryProjections(space: string) {
		const due = await this.deps.db
			.selectFrom('pendingProjection as pending')
			.leftJoin('spaceRecord as record', (join) =>
				join
					.onRef('record.spaceUri', '=', 'pending.spaceUri')
					.onRef('record.repoDid', '=', 'pending.repoDid')
					.onRef('record.collection', '=', 'pending.collection')
					.onRef('record.rkey', '=', 'pending.rkey')
			)
			.select([
				'pending.repoDid',
				'pending.collection',
				'pending.rkey',
				'pending.attempts',
				'record.cid',
				'record.value'
			])
			.where('pending.spaceUri', '=', space)
			.where('pending.nextAttemptAt', '<=', nowIso())
			.execute();

		await this.project(
			due.map(({ repoDid, collection, rkey, attempts, cid, value }) => {
				const path = { space, repoDid, collection, rkey, uri: spaceRecordUri(space, repoDid, collection, rkey) };

				return {
					attempts,
					change:
						cid === null
							? { kind: 'delete', ...path }
							: { kind: 'upsert', ...path, cid, value: parseJson<JsonObject>(value, {}) }
				};
			})
		);
	}

	private async dropSpace(space: string) {
		const { db } = this.deps;

		const records = await db
			.selectFrom('spaceRecord')
			.select(['repoDid', 'collection', 'rkey'])
			.where('spaceUri', '=', space)
			.execute();

		// A delete that never got projected has no record left to find it by.
		const pending = await db
			.selectFrom('pendingProjection')
			.select(['repoDid', 'collection', 'rkey'])
			.where('spaceUri', '=', space)
			.execute();

		for (const record of [...records, ...pending]) {
			await this.deps
				.onChange({
					kind: 'delete',
					space,
					...record,
					uri: spaceRecordUri(space, record.repoDid, record.collection, record.rkey)
				})
				.catch(() => undefined);
		}

		await db.deleteFrom('spaceRecord').where('spaceUri', '=', space).execute();
		await db.deleteFrom('syncedRepo').where('spaceUri', '=', space).execute();
		await db.deleteFrom('syncRepoRetry').where('spaceUri', '=', space).execute();
		await db.deleteFrom('pendingProjection').where('spaceUri', '=', space).execute();
		await db.deleteFrom('spaceWatch').where('spaceUri', '=', space).execute();
		await this.deps.onSpaceDropped(space);
	}

	/**
	 * Syncs a space with a fresh credential, renewing its registration for updates. A host's notice that the space was
	 * deleted isn't proof on its own, so the authority confirms it: it refuses the credential with SpaceDeleted, and the
	 * pass drops the local copy. A repeat finds no watch left. A space recreated under the same URI is new: watching it
	 * again starts from scratch.
	 */
	async reauthorize(space: string): Promise<void> {
		this.deps.credentials.invalidate(space);
		await this.sync(space);
	}

	async status(): Promise<SyncStatus[]> {
		const watches = await this.deps.db.selectFrom('spaceWatch').selectAll().orderBy('createdAt').execute();

		const synced = await this.deps.db
			.selectFrom('syncedRepo')
			.select(['spaceUri', 'repoDid', 'rev', 'lastSyncedAt', 'lastError'])
			.execute();

		// A repo that never synced has no syncedRepo row, only its retry, which is all that shows why.
		const unsynced = await this.deps.db
			.selectFrom('syncRepoRetry as retry')
			.leftJoin('syncedRepo as synced', (join) =>
				join.onRef('synced.spaceUri', '=', 'retry.spaceUri').onRef('synced.repoDid', '=', 'retry.repoDid')
			)
			.where('synced.repoDid', 'is', null)
			.select(['retry.spaceUri', 'retry.repoDid', 'retry.lastError'])
			.execute();

		const repos = [...synced, ...unsynced.map((repo) => ({ ...repo, rev: null, lastSyncedAt: null }))];

		// A row with no attempts is only waiting for its first run, which isn't a failure.
		const stuck = await this.deps.db
			.selectFrom('pendingProjection')
			.selectAll()
			.where('attempts', '>', 0)
			.orderBy('nextAttemptAt')
			.execute();

		return watches.map((watch) => ({
			space: watch.spaceUri,
			type: watch.spaceType,
			skey: watch.skey,
			spaceRev: watch.spaceRev,
			lastSyncedAt: watch.lastSyncedAt,
			lastError: watch.lastError,
			registrationExpiresAt: watch.registrationExpiresAt,
			repos: repos
				.values()
				.filter((repo) => repo.spaceUri === watch.spaceUri)
				.map((repo) => ({
					did: repo.repoDid,
					rev: repo.rev,
					lastSyncedAt: repo.lastSyncedAt,
					lastError: repo.lastError
				}))
				.toArray(),
			stuck: stuck
				.values()
				.filter((record) => record.spaceUri === watch.spaceUri)
				.map(({ repoDid, collection, rkey, attempts, nextAttemptAt, lastError }) => ({
					did: repoDid,
					collection,
					rkey,
					attempts,
					nextAttemptAt,
					lastError
				}))
				.toArray()
		}));
	}
}
