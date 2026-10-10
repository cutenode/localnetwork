import type { JsonObject, JsonValue } from '@atproto/lex-json';
import { z } from 'zod';
import { COLLECTION } from '#lib/nsid.ts';
import { parseMixReportText } from '#lib/mix/report.ts';
import type {
	ClaimEvidence,
	ProjectDetails,
	ProjectMix,
	ProjectProfile,
	ProjectView,
	RequestKind,
	RequestRecord,
	RequestStatus,
	RequestView,
	Viewer
} from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import type { RepoClient } from '../atproto/repo-client';
import { parseSpaceRecordUri } from '../atproto/spaces';
import { type Core, ForbiddenError, NotFoundError, UserError } from '../core';
import { nowIso, parseJson } from '../db';
import type { RequestRow } from '../db/schema';
import { diff } from './diff';
import type { ProjectService } from './projects';

export const DECISION_OUTCOMES = ['approved', 'rejected', 'changesRequested'] as const;

export type DecisionOutcome = (typeof DECISION_OUTCOMES)[number];

/** A computer.localnetwork.moderation.decision record. Synced ones are parsed with this before they apply. */
const decisionRecord = z.object({
	request: z.string(),
	requestCid: z.string().optional(),
	requester: z.string(),
	outcome: z.enum(DECISION_OUTCOMES),
	reason: z.string().optional(),
	decidedBy: z.string(),
	project: z.string().optional(),
	projectRevision: z.number().optional(),
	createdAt: z.string()
});

type DecisionRecord = z.infer<typeof decisionRecord>;

export type SubmitInput = {
	actor: Viewer;
	kind: RequestKind;
	projectUri?: string;
	profile?: ProjectProfile;
	details?: ProjectDetails;
	claim?: ClaimEvidence;
	message?: string;
	avatar?: { bytes: Uint8Array; mimeType: string };
	/** New mix tracklists, aligned with `profile.mixes` (see `withTracklists`). */
	tracklists?: ({ bytes: Uint8Array; mimeType: string } | undefined)[];
};

const MAX_PENDING_CREATES = 5;

const OUTCOME_STATUS: Record<DecisionOutcome, RequestStatus> = {
	approved: 'approved',
	rejected: 'rejected',
	changesRequested: 'changes_requested'
};

const KIND_LABEL: Record<RequestKind, string> = { create: 'new project', claim: 'claim', update: 'change request' };

/** The request lexicon leaves `kind` open (`knownValues`), so a valid record can still carry one this app doesn't handle. */
export const isRequestKind = (value: JsonValue | undefined): value is RequestKind =>
	Object.keys(KIND_LABEL).some((kind) => kind === value);

export class RequestService {
	constructor(
		private readonly core: Core,
		private readonly projects: ProjectService
	) {}

	/** The client to write as the actor, or null when the root must write on their behalf. */
	private async actorClient(actor: Viewer): Promise<RepoClient | null> {
		if (!actor.spacesCapable) return null;

		return this.core.userClient(actor.did);
	}

	async submit(input: SubmitInput): Promise<RequestView> {
		const { actor, kind } = input;
		const project = input.projectUri ? await this.projects.byUri(input.projectUri) : null;

		if (kind !== 'create' && !project) throw new NotFoundError('That project does not exist');

		if (kind === 'create') {
			if (!input.profile) throw new UserError('Fill in the project profile');

			const pending = await this.core.db
				.selectFrom('request')
				.select((eb) => eb.fn.countAll<number>().as('count'))
				.where('requesterDid', '=', actor.did)
				.where('kind', '=', 'create')
				.where('status', '=', 'pending')
				.executeTakeFirstOrThrow();

			if (Number(pending.count) >= MAX_PENDING_CREATES) {
				throw new UserError(`You already have ${MAX_PENDING_CREATES} new projects waiting for review`);
			}
		}

		if (kind === 'claim') {
			if (!input.claim?.evidence?.trim()) throw new UserError('Tell the moderators how to verify your claim');

			if (project!.managers.includes(actor.did)) throw new UserError('You already manage this project');

			if (project!.status === 'archived') throw new UserError('Archived projects cannot be claimed');

			const duplicate = await this.core.db
				.selectFrom('request')
				.select('uri')
				.where('requesterDid', '=', actor.did)
				.where('projectUri', '=', project!.uri)
				.where('kind', '=', 'claim')
				.where('status', 'in', ['pending', 'changes_requested'])
				.executeTakeFirst();

			if (duplicate) throw new UserError('You already have a claim waiting for review on this project');
		}

		if (kind === 'update') {
			if (!project!.managers.includes(actor.did))
				throw new ForbiddenError('Only managers of this project can propose changes');

			if (!input.profile && !input.details) throw new UserError('Nothing to change');

			if (await this.openUpdate(project!.uri, actor.did))
				throw new UserError('You already have changes waiting for review on this project');
		}

		const client = await this.actorClient(actor);
		const profile = input.profile ? await this.attachFiles(input.profile, input, client) : undefined;

		const record: RequestRecord = { kind, createdAt: nowIso() };

		if (project) {
			record.project = project.uri;
			record.baseRevision = project.revision;
		}

		if (profile) record.profile = profile;

		// Empty details are kept: on an update they clear the published ones, as in revise.
		if (input.details) record.details = input.details;

		if (input.claim) record.claim = input.claim;
		const message = input.message?.trim();

		if (message) record.message = message;

		if (!client) record.onBehalfOf = actor.did;
		const validated = validateRecord(COLLECTION.request, record);
		const writer = client ?? (await this.core.root());

		const ref = await writer.createSpaceRecord({
			space: this.core.spaces.moderation,
			collection: COLLECTION.request,
			record: validated
		});

		await this.index({ uri: ref.uri, cid: ref.cid, repoDid: writer.did, value: record });

		const changes =
			kind === 'update' && project
				? diff({ profile: project.profile }, { profile: profile ?? project.profile })
				: kind === 'create' && profile
					? diff({}, { profile })
					: [];

		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'request.submit',
				subject: ref.uri,
				subjectKind: 'request',
				summary: `Submitted a ${KIND_LABEL[kind]}${project ? ` for ${project.profile.name}` : profile ? ` for ${profile.name}` : ''}`,
				changes,
				details: { kind, project: project?.uri, onBehalf: !client, relationship: input.claim?.relationship }
			},
			{ actorClient: client }
		);
		const view = await this.get(ref.uri);

		if (!view) throw new Error('Request vanished after submitting');

		return view;
	}

	/** Uploads a new avatar and mix tracklists to the writer's repo and references them. */
	private async attachFiles(
		profile: ProjectProfile,
		files: Pick<SubmitInput, 'avatar' | 'tracklists'>,
		client: RepoClient | null
	): Promise<ProjectProfile> {
		if (!files.avatar && !files.tracklists?.some(Boolean)) return profile;
		const writer = client ?? (await this.core.root());
		let next = profile;

		if (files.avatar) next = { ...next, avatar: await writer.uploadBlob(files.avatar.bytes, files.avatar.mimeType) };

		if (next.mixes && files.tracklists?.some(Boolean)) {
			const mixes = [...next.mixes];

			for (const [index, file] of files.tracklists.entries()) {
				if (file && mixes[index])
					mixes[index] = { ...mixes[index], tracklist: await writer.uploadBlob(file.bytes, file.mimeType) };
			}

			next = { ...next, mixes };
		}

		return next;
	}

	/** Lets the requester revise a pending request or answer a "changes requested" decision. */
	async revise(
		actor: Viewer,
		uri: string,
		input: Omit<SubmitInput, 'actor' | 'kind' | 'projectUri'>
	): Promise<RequestView> {
		const row = await this.row(uri);

		if (row.requesterDid !== actor.did) throw new ForbiddenError('Only the requester can edit this request');

		if (row.status !== 'pending' && row.status !== 'changes_requested')
			throw new UserError('This request has already been decided');
		const current = storedRecord(row);
		const client = row.repoDid === actor.did ? await this.core.userClient(actor.did) : null;

		if (row.repoDid === actor.did && !client) throw new UserError('Sign in again to edit this request');
		const profile = input.profile ? await this.attachFiles(input.profile, input, client) : current.profile;
		const next: RequestRecord = { ...current, updatedAt: nowIso() };

		if (profile) next.profile = profile;

		if (input.details) next.details = input.details;

		if (input.claim) next.claim = input.claim;

		// A blank message clears the previous one.
		if (input.message !== undefined) {
			const message = input.message.trim();

			if (message) next.message = message;
			else delete next.message;
		}

		const validated = validateRecord(COLLECTION.request, next);
		const writer = client ?? (await this.core.root());

		const ref = await writer.putSpaceRecord({
			space: this.core.spaces.moderation,
			collection: COLLECTION.request,
			rkey: row.rkey,
			record: validated
		});

		await this.core.db.updateTable('request').set({ status: 'pending' }).where('uri', '=', uri).execute();
		await this.index({ uri: ref.uri, cid: ref.cid, repoDid: writer.did, value: next });
		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'request.update',
				subject: uri,
				subjectKind: 'request',
				summary: `Revised a ${KIND_LABEL[current.kind]}`,
				changes: diff(current, next).filter((change) => change.field !== 'updatedAt')
			},
			{ actorClient: client }
		);

		return (await this.get(uri))!;
	}

	async withdraw(actor: Viewer, uri: string): Promise<void> {
		const row = await this.row(uri);

		if (row.requesterDid !== actor.did) throw new ForbiddenError('Only the requester can withdraw this request');

		if (row.status !== 'pending' && row.status !== 'changes_requested')
			throw new UserError('This request has already been decided');
		const current = storedRecord(row);
		const client = row.repoDid === actor.did ? await this.core.userClient(actor.did) : null;
		const next: RequestRecord = { ...current, withdrawnAt: nowIso(), updatedAt: nowIso() };
		const writer = client ?? (await this.core.root());

		const ref = await writer.putSpaceRecord({
			space: this.core.spaces.moderation,
			collection: COLLECTION.request,
			rkey: row.rkey,
			record: validateRecord(COLLECTION.request, next)
		});

		await this.index({ uri: ref.uri, cid: ref.cid, repoDid: writer.did, value: next });
		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'request.withdraw',
				subject: uri,
				subjectKind: 'request',
				summary: `Withdrew a ${KIND_LABEL[current.kind]}`
			},
			{ actorClient: client }
		);
	}

	async decide(input: { actor: Viewer; uri: string; outcome: DecisionOutcome; reason?: string }): Promise<RequestView> {
		const { actor, outcome } = input;
		const reason = input.reason?.trim() || undefined;
		const row = await this.row(input.uri);

		const deny = async (message: string) => {
			await this.core.audit.record({
				actor: actor.did,
				actorRole: actorRole(actor),
				action:
					outcome === 'approved'
						? 'request.approve'
						: outcome === 'rejected'
							? 'request.reject'
							: 'request.changesRequested',
				subject: input.uri,
				subjectKind: 'request',
				summary: `Denied: ${message}`,
				outcome: 'denied',
				reason: message
			});
			throw new ForbiddenError(message);
		};

		if (!actor.isStaff) await deny('Only moderators and admins can review requests');

		if (row.requesterDid === actor.did) await deny("You can't review your own request");

		if (row.status !== 'pending') throw new UserError('This request is not waiting for review');

		if (outcome !== 'approved' && !reason) throw new UserError('Give the requester a reason');

		const record = storedRecord(row);
		const existing = row.projectUri ? await this.projects.byUri(row.projectUri) : null;
		let result: { uri: string; revision: number; name: string; changes: ReturnType<typeof diff> } | null = null;

		if (outcome === 'approved') {
			const profile = record.profile ? await this.copyFiles(record.profile, row.repoDid, existing) : undefined;

			if (record.kind === 'create') {
				if (!profile) throw new UserError('This request has no profile to publish');

				const { project, changes } = await this.projects.publish({
					profile,
					managers: [row.requesterDid],
					status: 'active',
					createdBy: row.requesterDid,
					details: record.details ?? undefined,
					lastRequest: row.uri
				});

				result = { uri: project.uri, revision: project.revision, name: project.profile.name, changes };
			} else {
				if (!existing) throw new UserError('The project no longer exists');

				if (record.kind === 'update' && !existing.managers.includes(row.requesterDid)) {
					throw new UserError('The requester no longer manages this project');
				}

				const managers = record.kind === 'claim' ? [...existing.managers, row.requesterDid] : existing.managers;

				if (managers.length > 10) throw new UserError('A project can have at most 10 managers');

				const { project, changes } = await this.projects.publish({
					existing,
					profile: record.kind === 'update' && profile ? profile : existing.profile,
					managers,
					status: existing.status,
					createdBy: existing.createdBy,
					details: record.kind === 'update' ? record.details : undefined,
					lastRequest: row.uri
				});

				result = { uri: project.uri, revision: project.revision, name: project.profile.name, changes };
			}
		}

		const root = await this.core.root();
		const decidedAt = nowIso();

		const decision: DecisionRecord = {
			request: row.uri,
			requestCid: row.cid,
			requester: row.requesterDid,
			outcome,
			decidedBy: actor.did,
			createdAt: decidedAt
		};

		if (reason) decision.reason = reason;

		if (result) {
			decision.project = result.uri;
			decision.projectRevision = result.revision;
		}

		const validated = validateRecord(COLLECTION.decision, decision);

		const ref = await root.createSpaceRecord({
			space: this.core.spaces.moderation,
			collection: COLLECTION.decision,
			record: validated
		});

		await this.applyDecision({ uri: ref.uri, value: validated });

		const subjectName = result?.name ?? existing?.profile.name ?? record.profile?.name ?? 'a project';
		const requester = await this.core.accounts.view(row.requesterDid);
		const who = requester.handle ? `@${requester.handle}` : row.requesterDid;
		const verb = outcome === 'approved' ? 'Approved' : outcome === 'rejected' ? 'Rejected' : 'Requested changes to';
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action:
				outcome === 'approved'
					? 'request.approve'
					: outcome === 'rejected'
						? 'request.reject'
						: 'request.changesRequested',
			subject: row.uri,
			subjectKind: 'request',
			summary: `${verb} ${who}'s ${KIND_LABEL[record.kind]} for ${subjectName}`,
			reason,
			details: {
				kind: record.kind,
				requester: row.requesterDid,
				decision: ref.uri,
				project: result?.uri,
				revision: result?.revision
			}
		});

		if (result) {
			await this.core.audit.record({
				actor: actor.did,
				actorRole: actorRole(actor),
				action:
					record.kind === 'create' ? 'project.create' : record.kind === 'claim' ? 'project.managers' : 'project.update',
				subject: result.uri,
				subjectKind: 'project',
				summary:
					record.kind === 'create'
						? `Published ${result.name} (revision ${result.revision})`
						: record.kind === 'claim'
							? `Added ${who} as a manager of ${result.name}`
							: `Updated ${result.name} to revision ${result.revision}`,
				changes: result.changes,
				details: { request: row.uri, revision: result.revision }
			});
		}

		return (await this.get(row.uri))!;
	}

	/**
	 * Moves the avatar and mix tracklists the requester uploaded to their own PDS into the root
	 * account's repo, so the published record references blobs root hosts. Blobs the published
	 * project already has (kept unchanged in an update) are root's already and stay as they are.
	 */
	private async copyFiles(
		profile: ProjectProfile,
		repoDid: string,
		existing: ProjectView | null
	): Promise<ProjectProfile> {
		if (repoDid === this.core.rootDid) return profile;

		const published = new Set(
			[existing?.profile.avatar, ...(existing?.profile.mixes ?? []).map((m) => m.tracklist)]
				.values()
				.filter((blob) => blob !== undefined)
				.map((blob) => blob.ref.$link)
		);

		let next =
			profile.avatar && published.has(profile.avatar.ref.$link) ? profile : await this.copyAvatar(profile, repoDid);

		if (next.mixes?.some((m) => m.tracklist)) {
			const mixes = [];

			for (const mix of next.mixes) mixes.push(await this.copyTracklist(mix, repoDid, published));
			next = { ...next, mixes };
		}

		return next;
	}

	private async copyTracklist(mix: ProjectMix, repoDid: string, published: Set<string>): Promise<ProjectMix> {
		const tracklist = mix.tracklist;

		if (!tracklist || published.has(tracklist.ref.$link)) return mix;
		const { tracklist: _dropped, ...without } = mix;

		if (!this.core.fetchSpaceBlob) {
			this.core.logger.warn('cannot copy a tracklist: no space blob fetcher configured');

			return without;
		}

		try {
			const { bytes } = await this.core.fetchSpaceBlob(this.core.spaces.moderation, repoDid, tracklist.ref.$link);
			// Re-check what is about to be published; the requester's PDS could serve anything.
			const parsed = parseMixReportText(bytes);

			if (!parsed.ok) throw new Error(parsed.error);
			const root = await this.core.root();
			const blob = await root.uploadBlob(new TextEncoder().encode(JSON.stringify(parsed.report)), 'application/json');

			return { ...mix, tracklist: blob };
		} catch (error) {
			this.core.logger.warn({ err: error, repoDid }, 'could not copy a tracklist; publishing the mix without it');

			return without;
		}
	}

	/**
	 * Moves an avatar the requester uploaded to their own PDS into the root
	 * account's repo, so the published record references a blob root hosts.
	 */
	private async copyAvatar(profile: ProjectProfile, repoDid: string): Promise<ProjectProfile> {
		const avatar = profile.avatar;

		if (!avatar || repoDid === this.core.rootDid) return profile;

		if (!this.core.fetchSpaceBlob) {
			this.core.logger.warn('cannot copy avatar: no space blob fetcher configured');
			const { avatar: _dropped, ...rest } = profile;

			return rest;
		}

		try {
			const { bytes, mimeType } = await this.core.fetchSpaceBlob(
				this.core.spaces.moderation,
				repoDid,
				avatar.ref.$link
			);

			const root = await this.core.root();
			const blob = await root.uploadBlob(bytes, mimeType || avatar.mimeType);

			return { ...profile, avatar: blob };
		} catch (error) {
			this.core.logger.warn({ err: error, repoDid }, 'could not copy avatar; publishing without it');
			const { avatar: _dropped, ...rest } = profile;

			return rest;
		}
	}

	/**
	 * Indexes a request record from our own write or from sync. Only the root
	 * may file on someone's behalf; `onBehalfOf` from anyone else is ignored.
	 */
	async index(input: { uri: string; cid: string; repoDid: string; value: RequestRecord }): Promise<void> {
		const parts = parseSpaceRecordUri(input.uri);

		if (!parts) return;

		const requester =
			input.repoDid === this.core.rootDid && input.value.onBehalfOf ? input.value.onBehalfOf : input.repoDid;

		const existing = await this.core.db
			.selectFrom('request')
			.selectAll()
			.where('uri', '=', input.uri)
			.executeTakeFirst();

		let status: RequestStatus = existing?.status ?? 'pending';

		if (input.value.withdrawnAt) status = 'withdrawn';
		else if (
			existing?.status === 'changes_requested' &&
			input.value.updatedAt &&
			existing.decidedAt &&
			input.value.updatedAt > existing.decidedAt
		) {
			status = 'pending';
		}

		const values = {
			cid: input.cid,
			repoDid: input.repoDid,
			requesterDid: requester,
			rkey: parts.rkey,
			kind: input.value.kind,
			projectUri: input.value.project ?? null,
			status,
			record: JSON.stringify(input.value),
			createdAt: input.value.createdAt,
			updatedAt: input.value.updatedAt ?? input.value.createdAt
		};

		await this.core.db
			.insertInto('request')
			.values({
				uri: input.uri,
				...values,
				decisionUri: null,
				decidedBy: null,
				decidedAt: null,
				decisionReason: null,
				resultProjectUri: null
			})
			.onConflict((oc) => oc.column('uri').doUpdateSet(values))
			.execute();
	}

	/** Applies a decision record (root-authored only) to its request. */
	async applyDecision(input: { uri: string; value: JsonObject }): Promise<void> {
		const parsed = decisionRecord.safeParse(input.value);

		if (!parsed.success) return;
		const value = parsed.data;

		const row = await this.core.db
			.selectFrom('request')
			.select(['decidedAt', 'status'])
			.where('uri', '=', value.request)
			.executeTakeFirst();

		if (!row || (row.decidedAt && row.decidedAt > value.createdAt)) return;

		if (row.status === 'withdrawn') return;
		await this.core.db
			.updateTable('request')
			.set({
				status: OUTCOME_STATUS[value.outcome],
				decisionUri: input.uri,
				decidedBy: value.decidedBy,
				decidedAt: value.createdAt,
				decisionReason: value.reason ?? null,
				resultProjectUri: value.project ?? null
			})
			.where('uri', '=', value.request)
			.execute();
	}

	private async row(uri: string): Promise<RequestRow> {
		const row = await this.core.db.selectFrom('request').selectAll().where('uri', '=', uri).executeTakeFirst();

		if (!row) throw new NotFoundError('Request not found');

		return row;
	}

	async get(uri: string): Promise<RequestView | null> {
		const row = await this.core.db.selectFrom('request').selectAll().where('uri', '=', uri).executeTakeFirst();

		if (!row) return null;

		return (await this.toViews([row]))[0];
	}

	/**
	 * Someone's open change request for a project. Each starts from the published profile, so there's
	 * at most one per manager: approving a second would undo the first.
	 */
	async openUpdate(projectUri: string, requesterDid: string): Promise<RequestView | null> {
		const [open] = await this.list({ kind: 'update', projectUri, requester: requesterDid, status: 'open', limit: 1 });

		return open ?? null;
	}

	async byRkey(rkey: string): Promise<RequestView | null> {
		const row = await this.core.db.selectFrom('request').selectAll().where('rkey', '=', rkey).executeTakeFirst();

		return row ? (await this.toViews([row]))[0] : null;
	}

	async list(
		filter: {
			status?: RequestStatus | 'open';
			requester?: string;
			kind?: RequestKind;
			projectUri?: string;
			limit?: number;
		} = {}
	): Promise<RequestView[]> {
		let query = this.core.db.selectFrom('request').selectAll();

		if (filter.status === 'open') query = query.where('status', 'in', ['pending', 'changes_requested']);
		else if (filter.status) query = query.where('status', '=', filter.status);

		if (filter.requester) query = query.where('requesterDid', '=', filter.requester);

		if (filter.kind) query = query.where('kind', '=', filter.kind);

		if (filter.projectUri) query = query.where('projectUri', '=', filter.projectUri);
		const order = filter.status === 'pending' ? 'asc' : 'desc';

		const rows = await query
			.orderBy('createdAt', order)
			.limit(Math.min(filter.limit ?? 100, 500))
			.execute();

		return this.toViews(rows);
	}

	async counts(): Promise<Record<RequestStatus, number>> {
		const rows = await this.core.db
			.selectFrom('request')
			.select(['status', (eb) => eb.fn.countAll<number>().as('count')])
			.groupBy('status')
			.execute();

		const counts: Record<RequestStatus, number> = {
			pending: 0,
			approved: 0,
			rejected: 0,
			changes_requested: 0,
			withdrawn: 0
		};

		for (const row of rows) counts[row.status] = Number(row.count);

		return counts;
	}

	private async toViews(rows: RequestRow[]): Promise<RequestView[]> {
		const accounts = await this.core.accounts.views(rows.flatMap((row) => [row.requesterDid, row.decidedBy ?? '']));

		const projects = await this.projects.byUris([
			...new Set(rows.flatMap((row) => [row.projectUri, row.resultProjectUri]).filter((uri): uri is string => !!uri))
		]);

		return rows.map((row) => {
			const project = row.projectUri ? projects.get(row.projectUri) : undefined;
			const result = row.resultProjectUri ? projects.get(row.resultProjectUri) : undefined;
			const record = storedRecord(row);

			return {
				uri: row.uri,
				cid: row.cid,
				rkey: row.rkey,
				kind: record.kind,
				status: row.status,
				requester: accounts.get(row.requesterDid) ?? { did: row.requesterDid, handle: null },
				repoDid: row.repoDid,
				projectUri: row.projectUri,
				project: project ? { slug: project.slug, name: project.profile.name } : null,
				record,
				createdAt: row.createdAt,
				updatedAt: row.updatedAt,
				decision: row.decidedAt
					? {
							by: row.decidedBy ? (accounts.get(row.decidedBy) ?? null) : null,
							at: row.decidedAt,
							reason: row.decisionReason
						}
					: null,
				resultProjectSlug: result?.slug ?? null
			};
		});
	}
}

/** The record a request row was indexed from; `index` stores it as JSON, so it always parses. */
function storedRecord(row: RequestRow): RequestRecord {
	const record = parseJson<RequestRecord | null>(row.record, null);

	if (!record) throw new Error(`Request ${row.uri} has no stored record`);

	return record;
}

export function actorRole(viewer: Viewer): 'admin' | 'moderator' | 'member' {
	return viewer.isAdmin ? 'admin' : viewer.isStaff ? 'moderator' : 'member';
}
