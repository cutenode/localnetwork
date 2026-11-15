import { ComAtprotoSpaceDefs } from '@atproto/api';
import { type Cid, isLexMap, isObject, type LexMap, parseCid } from '@atproto/lex-data';
import { type JsonObject, type JsonValue, jsonToLex, lexToJson } from '@atproto/lex-json';
import { isNsidString, isSpaceKeyString } from '@atproto/oauth-scopes';
import {
	COMMIT_MAC_TYPE,
	type Commit,
	type RecordPathParts,
	readRepoStar,
	RepoCommit,
	SpaceMacError,
	type VerifiedRecord,
	verifyCommit
} from '@atproto/space';
import PQueue from 'p-queue';
import { z } from 'zod';
import {
	type SpaceCredentialManager,
	isCredentialRejected,
	isMacFailure,
	XrpcResponseError
} from '../atproto/credentials';
import type { IdentityResolver } from '../atproto/identity';
import { isTooDeep, TOO_DEEP } from '../atproto/lexicons';
import { parseSpaceUri, spaceRecordUri } from '../atproto/spaces';
import { type Db, nowIso, parseJson } from '../db';
import type { Logger } from '../logger';

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

const listReposOutput = z.object({
	repos: z.array(z.object({ did: z.string(), repoRev: z.string(), spaceRev: z.string() })),
	cursor: z.string().optional()
});

type RepoEntry = z.infer<typeof listReposOutput>['repos'][number];

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
	commit: z.custom<Commit>((value) => ComAtprotoSpaceDefs.validateCommit(value).success).optional(),
	cursor: z.string().optional()
});

const registerNotifyOutput = z.object({ expiresAt: z.string().optional() });

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
 * A listRepoOps page decoded from its JSON, with the indexes of the ops whose values are nested deeper than
 * validateRecord allows. jsonToLex recurses, so those values are taken out before it decodes the rest.
 */
function readOpsPage(json: JsonValue) {
	const tooDeep = new Set<number>();
	const ops = isObject(json) && !Array.isArray(json) && Array.isArray(json.ops) ? json.ops : [];

	for (const [index, op] of ops.entries()) {
		if (!isObject(op) || Array.isArray(op) || !isTooDeep(op.value)) continue;
		tooDeep.add(index);
		delete op.value;
	}

	return { page: listRepoOpsOutput.parse(jsonToLex(json)), tooDeep };
}

// SAFETY: lexToJson only rewrites the CIDs and bytes inside a map, so a LexMap always comes back as a JSON object.
const toJson = (record: LexMap) => lexToJson(record) as JsonObject;

const asCid = (value: string | null): Cid | null => (value ? parseCid(value) : null);

/**
 * Whether the space's authority said the space is gone: getSpaceCredential and listRepos only go to the
 * authority. A writer's repo host answering SpaceDeleted says nothing about the space, and neither does a
 * credential renewal that fails any other way.
 */
function authoritySaysGone(error: XrpcResponseError): boolean {
	if (error.nsid === 'com.atproto.space.getSpaceCredential') return error.error === 'SpaceDeleted';

	return (
		error.nsid === 'com.atproto.space.listRepos' && (error.error === 'SpaceDeleted' || error.error === 'SpaceNotFound')
	);
}

/** Getting a credential fails for every repo in the space, so it ends the pass instead of counting against one repo. */
const failsWholeSpace = (cause: unknown) =>
	cause instanceof XrpcResponseError && cause.nsid === 'com.atproto.space.getSpaceCredential';

const REPO_UNAVAILABLE = new Set(['RepoTakendown', 'RepoSuspended', 'RepoDeactivated']);

/**
 * Whether a writer's repo host won't serve the repo because the account is taken down, suspended, or
 * deactivated. A full download fails the same way, and the pass carries on without it: the repo waits out its
 * backoff, so it's picked up again once the account is back. The same errors from getSpaceCredential mean the
 * authority's account is, which suspends the whole space.
 */
const isRepoUnavailable = (cause: unknown) =>
	cause instanceof XrpcResponseError && REPO_UNAVAILABLE.has(cause.error) && !failsWholeSpace(cause);

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
 * `listRepos` on the space host, pulls each repo's oplog with
 * `listRepoOps`, checks the result against the LtHash commit (authenticated
 * by a MAC keyed to the author's `#atproto_kem` key), and falls back to a
 * full verified STAR download (`getRepo`) when the incremental path cannot be
 * trusted. A repo that fails doesn't hold back the rest: the pass carries on,
 * its cursor moves past the repo, and `syncRepoRetry` keeps it for a retry,
 * since `listRepos` won't list it again until it next changes. A record is
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
			/** Service id to register for write notifications, or null to rely on polling. */
			notifyService: string | null;
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
			await this.maybeRegister(space, watch.authorityDid, authority.pds, watch.registrationExpiresAt);
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

			for (;;) {
				const page = await this.deps.credentials.query({
					space,
					service: authority.pds,
					audience: watch.authorityDid,
					nsid: 'com.atproto.space.listRepos',
					params: { space, limit: 500, cursor },
					schema: listReposOutput
				});

				const last = page.repos.at(-1);

				if (!last) break;

				await settle(page.repos.map((repo) => this.repoQueue.add(() => this.syncRepoIfBehind(space, repo))));
				cursor = page.cursor ?? last.spaceRev;

				if (!page.cursor) break;
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
				.set({ spaceRev: cursor ?? null, lastSyncedAt: nowIso(), lastError: waiting?.lastError ?? null })
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

	private async maybeRegister(space: string, authorityDid: string, host: string, expiresAt: string | null) {
		if (!this.deps.notifyService) return;

		if (expiresAt && Date.parse(expiresAt) - Date.now() > 10 * 60_000) return;

		try {
			const body = await this.deps.credentials.procedure({
				space,
				service: host,
				audience: authorityDid,
				nsid: 'com.atproto.space.registerNotify',
				body: { space, service: this.deps.notifyService },
				schema: registerNotifyOutput
			});

			await this.deps.db
				.updateTable('spaceWatch')
				.set({ registrationExpiresAt: body.expiresAt ?? null })
				.where('spaceUri', '=', space)
				.execute();
		} catch (error) {
			this.deps.logger.debug('could not register for write notifications; polling instead', { err: error, space });
		}
	}

	private async syncRepoIfBehind(space: string, repo: RepoEntry): Promise<boolean> {
		const local = await this.deps.db
			.selectFrom('syncedRepo')
			.select(['rev'])
			.where('spaceUri', '=', space)
			.where('repoDid', '=', repo.did)
			.executeTakeFirst();

		if (local && local.rev >= repo.repoRev) return false;

		const retry = await this.deps.db
			.selectFrom('syncRepoRetry')
			.select('nextAttemptAt')
			.where('spaceUri', '=', space)
			.where('repoDid', '=', repo.did)
			.executeTakeFirst();

		if (retry && retry.nextAttemptAt > nowIso()) return false;
		await this.syncRepo(space, repo.did);

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
		let commit: Commit | undefined;

		do {
			// The host wants a MAC request on every page, and only MACs the pages that carry a commit.
			const { response, macParams } = await this.deps.credentials.macRequest({
				space,
				service: pds,
				audience: repoDid,
				nsid: 'com.atproto.space.listRepoOps',
				params: { space, repo: repoDid, since: cursor ? undefined : since, cursor, limit: 500 },
				mac: { type: COMMIT_MAC_TYPE, kemKey }
			});

			const json: JsonValue = JSON.parse(await response.text());
			const { page, tooDeep } = readOpsPage(json);

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

			if (page.commit) {
				if (!verifyCommit(page.commit, { space, repo: repoDid, ...macParams() })) {
					throw new SpaceMacError('Commit does not match its MAC');
				}

				commit = page.commit;
			}

			cursor = page.cursor;
		} while (cursor);

		if (!commit) {
			if (changes.length === 0) return;
			throw new Error('Oplog did not end with a commit');
		}

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
		const { response, macParams } = await this.deps.credentials.macRequest({
			space,
			service: pds,
			audience: repoDid,
			nsid: 'com.atproto.space.getRepo',
			params: { space, repo: repoDid },
			mac: { type: COMMIT_MAC_TYPE, kemKey }
		});

		if (!response.body) throw new Error('Empty repo response');

		// A STAR archive: the commit, then every record, which must fold into its data. verifyRepoStar would check the
		// commit too, but reports a MAC that doesn't verify as the same RepoVerificationError as an archive that
		// doesn't parse, and only the first is worth refreshing the KEM key for.
		const archive = await readRepoStar(response.body);
		const records: VerifiedRecord[] = [];

		try {
			if (!verifyCommit(archive.commit, { space, repo: repoDid, ...macParams() })) {
				throw new SpaceMacError('Commit does not match its MAC');
			}

			for await (const record of archive.records) records.push(record);
		} finally {
			await archive[Symbol.asyncDispose]();
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

		for (const record of records) {
			const path = `${record.collection}/${record.rkey}`;
			present.add(path);
			const cid = record.cid.toString();

			if (known.get(path) === cid) continue;
			changes.push({
				kind: 'upsert',
				space,
				repoDid,
				collection: record.collection,
				rkey: record.rkey,
				uri: spaceRecordUri(space, repoDid, record.collection, record.rkey),
				cid,
				value: toJson(record.record)
			});
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

		await this.apply(space, repoDid, pds, archive.commit, RepoCommit.fromRecords(records), changes);
	}

	private async apply(
		space: string,
		repoDid: string,
		pds: string,
		commit: Commit,
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
	 * Handles a verified `notifySpaceDeleted`. Authorities retry it until it's answered, so a repeat finds
	 * nothing left to drop. A space recreated under the same URI is new: watching it again starts from scratch.
	 */
	async onSpaceDeleted(space: string): Promise<void> {
		this.deps.credentials.invalidate(space);
		// No pass starts without the watch, and one already running could write the space back after the purge.
		await this.deps.db.deleteFrom('spaceWatch').where('spaceUri', '=', space).execute();

		for (let running = this.spaceLocks.get(space); running; running = this.spaceLocks.get(space)) {
			await running.catch(() => undefined);
		}

		await this.dropSpace(space);
		this.deps.logger.info('space deleted; dropped local copy', { space });
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
