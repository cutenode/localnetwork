import { TID } from '@atproto/common-web';
import type { JsonObject, JsonValue } from '@atproto/lex-json';
import { sql } from 'kysely';
import { z } from 'zod';
import { COLLECTION } from '#lib/nsid.ts';
import type { RepoClient, RootRepoClient } from '../atproto/repo-client';
import { xrpcErrorName } from '../atproto/repo-client';
import { validateRecord } from '../atproto/lexicons';
import { type Db, nowIso, parseJson } from '../db';
import type { AuditEntryRow, CopyStatus } from '../db/schema';
import type { Logger } from '../logger';
import { type AuditDetails, canonicalJson, chainHash, clip, sanitizeDetails } from './chain';

/** Every action the log records. The audit entry lexicon lists the same ones as its knownValues. */
export const AUDIT_ACTIONS = [
	'auth.login',
	'auth.logout',
	'project.create',
	'project.update',
	'project.status',
	'project.managers',
	'request.submit',
	'request.update',
	'request.withdraw',
	'request.approve',
	'request.reject',
	'request.changesRequested',
	'role.grant',
	'role.revoke',
	'verification.grant',
	'verification.revoke',
	'booking.create',
	'booking.message',
	'booking.status',
	'planner.save',
	'planner.unsave',
	'planner.show',
	'epk.save',
	'epk.delete',
	'epk.request',
	'epk.grant',
	'epk.decline',
	'epk.revoke',
	'show.save',
	'show.delete',
	'follow.create',
	'follow.delete',
	'space.create',
	'people.add',
	'people.update',
	'people.remove',
	'people.leave',
	'people.hide',
	'listing.list',
	'listing.unlist',
	'system.bootstrap',
	'system.space'
] as const;

export type AuditAction = (typeof AUDIT_ACTIONS)[number];

export type ActorRole = 'admin' | 'moderator' | 'member' | 'system';

/** What an entry's subject is. The audit entry lexicon lists the same ones as its knownValues. */
export const SUBJECT_KINDS = [
	'account',
	'project',
	'request',
	'role',
	'booking',
	'planner',
	'space',
	'epk',
	'show'
] as const;

export type SubjectKind = (typeof SUBJECT_KINDS)[number];

export type AuditChange = { field: string; before?: string; after?: string };

export type AuditEvent = {
	actor: string;
	actorRole?: ActorRole;
	action: AuditAction;
	subject?: string;
	subjectKind?: SubjectKind;
	summary: string;
	changes?: AuditChange[];
	details?: AuditDetails;
	outcome?: 'success' | 'failure' | 'denied';
	reason?: string;
	via?: 'web' | 'system' | 'sync';
};

export type AuditDeps = {
	db: Db;
	logger: Logger;
	auditSpace: () => string;
	rootDid: () => string;
	rootClient: () => Promise<RootRepoClient>;
	/** The acting account's own client (its OAuth session), or null if unavailable. */
	userClient: (did: string) => Promise<RepoClient | null>;
	/** Whether the account granted every scope the app asked for (so its PDS supports spaces). */
	spacesCapable: (did: string) => Promise<boolean>;
	maxAttempts: number;
};

export type AuditFilter = {
	actor?: string;
	action?: string;
	subject?: string;
	outcome?: string;
	query?: string;
	before?: number;
	limit?: number;
};

/** One copy of an entry as written to atproto (computer.localnetwork.audit.entry). */
type AuditRecord = {
	actor: string;
	actorRole?: string;
	action: string;
	subject?: string;
	subjectKind?: string;
	summary: string;
	changes?: AuditChange[];
	details?: JsonObject;
	outcome: string;
	reason?: string;
	via: string;
	chain?: { seq: number; prev: string; hash: string };
	createdAt: string;
};

/** The parts of a root copy that rebuild a lost local entry (missing fields get the fallbacks in `onSyncedCopy`). */
const rootCopy = z
	.object({
		actor: z.string(),
		actorRole: z.string(),
		action: z.string(),
		subject: z.string(),
		subjectKind: z.string(),
		summary: z.string(),
		outcome: z.string(),
		reason: z.string(),
		via: z.string(),
		createdAt: z.string()
	})
	.partial()
	.extend({ chain: z.object({ seq: z.int().min(1), prev: z.string(), hash: z.string() }) });

const backoffMs = (attempts: number) => Math.min(60 * 60_000, 5_000 * 2 ** Math.max(0, attempts - 1));

/**
 * The audit log. Every action is recorded locally first (an outbox with a
 * SHA-256 hash chain), then written to atproto twice with the same record
 * key: into the actor's own repo in the root's audit space, and into the root
 * account's repo in that space (with the chain fields). Failed writes retry
 * with exponential backoff until `maxAttempts`.
 */
export class AuditLog {
	private chainLock: Promise<unknown> = Promise.resolve();
	private readonly delivering = new Set<string>();
	private readonly inflight = new Set<Promise<void>>();

	constructor(private readonly deps: AuditDeps) {}

	async record(
		event: AuditEvent,
		options: { actorClient?: RepoClient | null; awaitDelivery?: boolean } = {}
	): Promise<AuditEntryRow> {
		const entry = await this.append(event);

		const delivery = this.deliver(entry.id, options.actorClient ?? undefined).catch((error) => {
			this.deps.logger.error({ err: error, id: entry.id }, 'audit delivery crashed');
		});

		this.inflight.add(delivery);
		void delivery.finally(() => this.inflight.delete(delivery));

		if (options.awaitDelivery) await delivery;

		return entry;
	}

	private append(event: AuditEvent): Promise<AuditEntryRow> {
		const run = this.chainLock.then(() => this.appendLocked(event));
		this.chainLock = run.catch(() => undefined);

		return run;
	}

	private async appendLocked(event: AuditEvent): Promise<AuditEntryRow> {
		const { db } = this.deps;
		const rootDid = this.deps.rootDid();

		const head = await db
			.selectFrom('auditEntry')
			.select(['seq', 'hash'])
			.orderBy('seq', 'desc')
			.limit(1)
			.executeTakeFirst();

		const seq = (head?.seq ?? 0) + 1;
		const prev = head?.hash ?? '';
		const id = TID.nextStr();
		const createdAt = nowIso();

		const changes = event.changes?.slice(0, 100).map((change) => ({
			field: clip(change.field, 128) ?? '',
			before: clip(change.before, 4000),
			after: clip(change.after, 4000)
		}));

		const sanitized = sanitizeDetails(event.details ?? {});
		const details = Object.keys(sanitized).length > 0 ? sanitized : undefined;

		const content = {
			id,
			seq,
			actor: event.actor,
			actorRole: event.actorRole ?? null,
			action: event.action,
			subject: event.subject ?? null,
			subjectKind: event.subjectKind ?? null,
			summary: clip(event.summary, 300) ?? '',
			changes: changes?.length ? changes : undefined,
			details,
			outcome: event.outcome ?? 'success',
			reason: clip(event.reason, 1000) ?? null,
			via: event.via ?? 'web',
			createdAt
		};

		const hash = chainHash(content, prev);

		let actorStatus: CopyStatus = 'pending';

		if (event.actor === rootDid) actorStatus = 'self';
		else if (!(await this.deps.spacesCapable(event.actor))) actorStatus = 'unsupported';

		const row: AuditEntryRow = {
			id,
			seq,
			actorDid: content.actor,
			actorRole: content.actorRole,
			action: content.action,
			subject: content.subject,
			subjectKind: content.subjectKind,
			summary: content.summary,
			changes: content.changes ? JSON.stringify(content.changes) : null,
			details: details ? JSON.stringify(details) : null,
			outcome: content.outcome,
			reason: content.reason,
			via: content.via,
			createdAt,
			prevHash: prev,
			hash,
			rootStatus: 'pending',
			rootUri: null,
			rootCid: null,
			rootAttempts: 0,
			rootError: null,
			actorStatus,
			actorUri: null,
			actorCid: null,
			actorAttempts: 0,
			actorError: null,
			nextAttemptAt: createdAt
		};

		await db.insertInto('auditEntry').values(row).execute();

		return row;
	}

	/** Builds the atproto record for one copy of an entry. */
	static toRecord(entry: AuditEntryRow, withChain: boolean) {
		const record: AuditRecord = {
			actor: entry.actorDid,
			action: entry.action,
			summary: entry.summary,
			outcome: entry.outcome,
			via: entry.via,
			createdAt: entry.createdAt
		};

		if (entry.actorRole) record.actorRole = entry.actorRole;

		if (entry.subject) record.subject = entry.subject;

		if (entry.subjectKind) record.subjectKind = entry.subjectKind;

		if (entry.reason) record.reason = entry.reason;
		const changes = parseJson<AuditChange[] | null>(entry.changes, null);

		if (changes?.length) record.changes = changes;
		const details = parseJson<JsonObject | null>(entry.details, null);

		if (details && Object.keys(details).length) record.details = details;

		if (withChain) record.chain = { seq: entry.seq, prev: entry.prevHash, hash: entry.hash };

		return validateRecord(COLLECTION.auditEntry, record);
	}

	/** Attempts any outstanding copies of one entry. Safe to call repeatedly. */
	async deliver(id: string, actorClient?: RepoClient): Promise<void> {
		if (this.delivering.has(id)) return;
		this.delivering.add(id);

		try {
			const entry = await this.deps.db.selectFrom('auditEntry').selectAll().where('id', '=', id).executeTakeFirst();

			if (!entry) return;
			const update: Partial<AuditEntryRow> = {};
			const space = this.deps.auditSpace();

			if (entry.rootStatus === 'pending') {
				try {
					const client = await this.deps.rootClient();
					const ref = await this.writeCopy(client, space, entry, true);
					Object.assign(update, { rootStatus: 'written', rootUri: ref.uri, rootCid: ref.cid, rootError: null });
				} catch (error) {
					const attempts = entry.rootAttempts + 1;
					Object.assign(update, {
						rootAttempts: attempts,
						rootError: errorMessage(error),
						rootStatus: attempts >= this.deps.maxAttempts ? 'failed' : 'pending'
					});
					this.deps.logger.warn({ err: error, id, attempts }, 'root audit copy failed');
				}
			}

			if (entry.actorStatus === 'pending') {
				try {
					const client = actorClient ?? (await this.deps.userClient(entry.actorDid));

					if (!client) throw new Error('No active session for the actor');
					const ref = await this.writeCopy(client, space, entry, false);
					Object.assign(update, { actorStatus: 'written', actorUri: ref.uri, actorCid: ref.cid, actorError: null });
				} catch (error) {
					const attempts = entry.actorAttempts + 1;
					Object.assign(update, {
						actorAttempts: attempts,
						actorError: errorMessage(error),
						actorStatus: attempts >= this.deps.maxAttempts ? 'failed' : 'pending'
					});
					this.deps.logger.warn({ err: error, id, attempts, actor: entry.actorDid }, 'actor audit copy failed');
				}
			}

			// Sync may have verified a copy while we were writing it; never downgrade.
			const current = await this.deps.db
				.selectFrom('auditEntry')
				.select(['rootStatus', 'actorStatus'])
				.where('id', '=', id)
				.executeTakeFirst();

			if (current?.rootStatus === 'verified') delete update.rootStatus;

			if (current?.actorStatus === 'verified') delete update.actorStatus;
			const rootPending = (update.rootStatus ?? entry.rootStatus) === 'pending';
			const actorPending = (update.actorStatus ?? entry.actorStatus) === 'pending';
			const attempts = Math.max(update.rootAttempts ?? entry.rootAttempts, update.actorAttempts ?? entry.actorAttempts);
			update.nextAttemptAt =
				rootPending || actorPending ? new Date(Date.now() + backoffMs(attempts)).toISOString() : null;
			await this.deps.db.updateTable('auditEntry').set(update).where('id', '=', id).execute();
		} finally {
			this.delivering.delete(id);
		}
	}

	private async writeCopy(client: RepoClient, space: string, entry: AuditEntryRow, withChain: boolean) {
		const record = AuditLog.toRecord(entry, withChain);

		try {
			return await client.createSpaceRecord({ space, collection: COLLECTION.auditEntry, rkey: entry.id, record });
		} catch (error) {
			if (xrpcErrorName(error) !== 'RecordAlreadyExists') throw error;

			const existing = await client.getSpaceRecord({
				space,
				repo: client.did,
				collection: COLLECTION.auditEntry,
				rkey: entry.id
			});

			if (!existing) throw error;

			return { uri: existing.uri, cid: existing.cid };
		}
	}

	/** Resolves once every delivery started so far has finished (for shutdown and tests). */
	async idle(): Promise<void> {
		while (this.inflight.size > 0) await Promise.allSettled(this.inflight);
	}

	/** Retries due deliveries. Returns how many entries were attempted. */
	async processPending(limit = 25): Promise<number> {
		const due = await this.deps.db
			.selectFrom('auditEntry')
			.select('id')
			.where('nextAttemptAt', 'is not', null)
			.where('nextAttemptAt', '<=', nowIso())
			.orderBy('seq')
			.limit(limit)
			.execute();

		for (const { id } of due) await this.deliver(id);

		return due.length;
	}

	async list(filter: AuditFilter = {}): Promise<AuditEntryRow[]> {
		let query = this.deps.db.selectFrom('auditEntry').selectAll();

		if (filter.actor) query = query.where('actorDid', '=', filter.actor);

		if (filter.action) query = query.where('action', 'like', `${filter.action.replace(/[%_]/g, '')}%`);

		if (filter.subject) query = query.where('subject', '=', filter.subject);

		if (filter.outcome) query = query.where('outcome', '=', filter.outcome);

		if (filter.before) query = query.where('seq', '<', filter.before);

		if (filter.query) {
			const like = `%${filter.query.replace(/[%_]/g, '')}%`;
			query = query.where((eb) =>
				eb.or([eb('summary', 'like', like), eb('subject', 'like', like), eb('actorDid', 'like', like)])
			);
		}

		return query
			.orderBy('seq', 'desc')
			.limit(Math.min(filter.limit ?? 50, 500))
			.execute();
	}

	async get(id: string): Promise<AuditEntryRow | undefined> {
		return this.deps.db.selectFrom('auditEntry').selectAll().where('id', '=', id).executeTakeFirst();
	}

	async stats() {
		const row = await this.deps.db
			.selectFrom('auditEntry')
			.select((eb) => [
				eb.fn.countAll<number>().as('total'),
				eb.fn
					.sum<number>(sql`CASE WHEN root_status = 'pending' OR actor_status = 'pending' THEN 1 ELSE 0 END`)
					.as('pending'),
				eb.fn
					.sum<number>(sql`CASE WHEN root_status = 'failed' OR actor_status = 'failed' THEN 1 ELSE 0 END`)
					.as('failed'),
				eb.fn.sum<number>(sql`CASE WHEN actor_status = 'unsupported' THEN 1 ELSE 0 END`).as('unsupported')
			])
			.executeTakeFirstOrThrow();

		return {
			total: Number(row.total ?? 0),
			pending: Number(row.pending ?? 0),
			failed: Number(row.failed ?? 0),
			unsupported: Number(row.unsupported ?? 0)
		};
	}

	/**
	 * Recomputes every hash and checks each link to its predecessor. Detects
	 * edited, reordered, or removed entries in the local log.
	 */
	async verifyChain(): Promise<{ ok: boolean; checked: number; brokenAt?: number; problem?: string }> {
		let prev = '';
		let expectedSeq = 1;
		let checked = 0;
		let lastSeq = 0;

		for (;;) {
			const batch = await this.deps.db
				.selectFrom('auditEntry')
				.selectAll()
				.where('seq', '>', lastSeq)
				.orderBy('seq')
				.limit(500)
				.execute();

			if (batch.length === 0) break;

			for (const entry of batch) {
				if (entry.seq !== expectedSeq) return { ok: false, checked, brokenAt: expectedSeq, problem: 'missing entry' };

				if (entry.prevHash !== prev) return { ok: false, checked, brokenAt: entry.seq, problem: 'broken link' };
				const recomputed = chainHash(entryContent(entry), entry.prevHash);

				if (recomputed !== entry.hash) return { ok: false, checked, brokenAt: entry.seq, problem: 'content changed' };
				prev = entry.hash;
				expectedSeq++;
				checked++;
				lastSeq = entry.seq;
			}
		}

		return { ok: true, checked };
	}

	/**
	 * Called by the space syncer for every audit record found in the audit
	 * space. Marks copies verified, and rebuilds missing local entries from the
	 * root's chained copies (so the local log can be recovered from atproto).
	 */
	async onSyncedCopy(input: {
		uri: string;
		repoDid: string;
		rkey: string;
		cid: string;
		value: JsonObject;
	}): Promise<void> {
		const { db } = this.deps;
		await db
			.insertInto('auditCopy')
			.values({
				uri: input.uri,
				repoDid: input.repoDid,
				rkey: input.rkey,
				cid: input.cid,
				record: canonicalJson(input.value),
				syncedAt: nowIso()
			})
			.onConflict((oc) =>
				oc.column('uri').doUpdateSet({ cid: input.cid, record: canonicalJson(input.value), syncedAt: nowIso() })
			)
			.execute();
		const entry = await this.get(input.rkey);
		const rootDid = this.deps.rootDid();

		if (entry) {
			const isRootCopy = input.repoDid === rootDid;
			const isActorCopy = !isRootCopy && input.repoDid === entry.actorDid;

			if (!isRootCopy && !isActorCopy) return;
			const expected = AuditLog.toRecord(entry, isRootCopy);

			const matches =
				canonicalJson({ ...expected, $type: undefined }) === canonicalJson({ ...input.value, $type: undefined });

			if (!matches) {
				// A copy that differs from the chained entry (edited after the fact) is flagged even if it was verified before.
				this.deps.logger.warn({ id: entry.id, uri: input.uri }, 'audit copy does not match the local entry');
				await db
					.updateTable('auditEntry')
					.set(
						isRootCopy
							? { rootStatus: 'failed', rootError: 'Root copy in the repo does not match this entry' }
							: { actorStatus: 'failed', actorError: "Actor's copy in their repo does not match this entry" }
					)
					.where('id', '=', entry.id)
					.execute();

				return;
			}

			const status = isRootCopy ? entry.rootStatus : entry.actorStatus;

			if (status === 'verified') return;
			await db
				.updateTable('auditEntry')
				.set(
					isRootCopy
						? { rootStatus: 'verified', rootUri: input.uri, rootCid: input.cid, rootError: null }
						: { actorStatus: 'verified', actorUri: input.uri, actorCid: input.cid, actorError: null }
				)
				.where('id', '=', entry.id)
				.execute();

			return;
		}

		if (input.repoDid !== rootDid) return;
		const copy = rootCopy.safeParse(input.value);

		if (!copy.success) return;
		const { chain, ...value } = copy.data;
		await db
			.insertInto('auditEntry')
			.values({
				id: input.rkey,
				seq: chain.seq,
				actorDid: value.actor ?? '',
				actorRole: value.actorRole ?? null,
				action: value.action ?? '',
				subject: value.subject ?? null,
				subjectKind: value.subjectKind ?? null,
				summary: value.summary ?? '',
				changes: input.value.changes ? JSON.stringify(input.value.changes) : null,
				details: input.value.details ? JSON.stringify(input.value.details) : null,
				outcome: value.outcome ?? 'success',
				reason: value.reason ?? null,
				via: value.via ?? 'web',
				createdAt: value.createdAt ?? nowIso(),
				prevHash: chain.prev,
				hash: chain.hash,
				rootStatus: 'verified',
				rootUri: input.uri,
				rootCid: input.cid,
				rootAttempts: 0,
				rootError: null,
				actorStatus: value.actor === rootDid ? 'self' : 'pending',
				actorUri: null,
				actorCid: null,
				actorAttempts: 0,
				actorError: null,
				nextAttemptAt: null
			})
			.onConflict((oc) => oc.doNothing())
			.execute();
	}

	/** Copies by non-root accounts that have no matching root entry. */
	async orphans(limit = 50) {
		return this.deps.db
			.selectFrom('auditCopy')
			.leftJoin('auditEntry', 'auditEntry.id', 'auditCopy.rkey')
			.select(['auditCopy.uri', 'auditCopy.repoDid', 'auditCopy.rkey', 'auditCopy.record', 'auditCopy.syncedAt'])
			.where('auditEntry.id', 'is', null)
			.orderBy('auditCopy.syncedAt', 'desc')
			.limit(limit)
			.execute();
	}
}

export function entryContent(entry: AuditEntryRow) {
	return {
		id: entry.id,
		seq: entry.seq,
		actor: entry.actorDid,
		actorRole: entry.actorRole,
		action: entry.action,
		subject: entry.subject,
		subjectKind: entry.subjectKind,
		summary: entry.summary,
		changes: parseJson<JsonValue | undefined>(entry.changes, undefined),
		details: parseJson<JsonValue | undefined>(entry.details, undefined),
		outcome: entry.outcome,
		reason: entry.reason,
		via: entry.via,
		createdAt: entry.createdAt
	};
}

function errorMessage(cause: unknown): string {
	const message = cause instanceof Error ? cause.message : String(cause);

	return message.length > 500 ? `${message.slice(0, 499)}…` : message;
}
