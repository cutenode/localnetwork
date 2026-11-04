import { TID } from '@atproto/common-web';
import type { z } from 'zod';
import { epkContentSchema, epkGrantSchema, epkRecordSchema, epkRequestSchema, followSchema } from '#lib/epk-schema.ts';
import type { JsonObject } from '@atproto/lex-json';
import {
	type AccessRule,
	AUDIENCES,
	type Audience,
	type EpkAccessState,
	type EpkContent,
	type EpkDashboard,
	type EpkGrantView,
	type EpkPhoto,
	type EpkRecord,
	type EpkRequestView,
	type EpkView,
	grantVia,
	LEGACY_KIT,
	MAX_EPKS_PER_PROJECT,
	readRule,
	REQUEST_ROLES,
	type RequestRole,
	ruleFor
} from '#lib/epk.ts';
import { BSKY_FOLLOW, COLLECTION, EPK_CONTENT_KEY, SPACE_TYPE } from '#lib/nsid.ts';
import type { ProjectView, Viewer } from '#lib/types.ts';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import { memberSpacePolicy, type RepoClient, type StoredRecord } from '../atproto/repo-client';
import {
	epkSpace,
	parsePublicRecordUri,
	parseSpaceRecordUri,
	parseSpaceUri,
	publicRecordUri,
	spaceOfRecord
} from '../atproto/spaces';
import { type Core, ForbiddenError, NotFoundError, UserError } from '../core';
import { nowIso } from '../db';
import type { EpkRequestRow } from '../db/schema';
import { EPK_PHOTO_MAX_BYTES } from '../forms/epk';
import type { DirectoryRecord } from '../happyview/client';
import type { IndexedRepo } from '../happyview/indexed';
import { EpkDrafts } from './epk-drafts';
import { cidOf, toJson } from './epk-fields';
import { type LegacyMigration, LegacyEpks } from './epk-legacy';
import { spaceWriter } from './own-repo';
import type { ProjectService } from './projects';
import { actorRole } from './requests';
import { ensureRootSpace } from './space-members';

/** How long the lists of press kits are reused. */
const FRESH_MS = 60_000;

const FOLLOWS_FRESH_MS = 10 * 60_000;

/** How long HappyView gets to re-read a manager's follows before the cached ones are dropped anyway. */
const FOLLOWS_REINDEX_MS = 60_000;

/** Pages of 100 follows read before giving up on finding someone; 5,000 follows. */
const FOLLOW_PAGES = 50;

/**
 * Whether a press kit's "people you follow" rule can differ from anyone's, the only case its project's managers'
 * follows are read in, and so the only projects whose managers' follows HappyView keeps (lua/app/bsky/graph/follow.lua
 * checks the same).
 */
export const readsFollows = (record: Pick<EpkRecord, 'access'>) => record.access.following !== record.access.anyone;

export type PressKit = { epk: EpkView; project: ProjectView };

/** A grant as its record holds it. */
export type GrantInput = { subject: string; via: 'auto' | 'approved' | 'invited'; request?: string; createdAt: string };

const newestFirst = (a: EpkView, b: EpkView) =>
	(b.record.updatedAt ?? b.record.createdAt).localeCompare(a.record.updatedAt ?? a.record.createdAt);

/**
 * A grant's record key in its project's request space: the press kit's key, then the subject's DID with every "%" as
 * "~". A record key can't hold a "%" and no DID holds a "~", so each press kit and subject gets a valid key of its own.
 */
export const grantKey = (kitRkey: string, subject: string): string => `${kitRkey}:${subject.replaceAll('%', '~')}`;

/** Deletes the record at `uri` in one of the root's spaces. */
export async function deleteAt(root: RepoClient, uri: string): Promise<void> {
	const { collection, rkey } = parseSpaceRecordUri(uri)!;
	await root.deleteSpaceRecord({ space: spaceOfRecord(uri)!, collection, rkey });
}

/** The press kit record a press kit's space belongs to: the one under the same key in its authority's repo. */
function kitOfSpace(space: string): string {
	const { authority, skey } = parseSpaceUri(space)!;

	return publicRecordUri(authority, COLLECTION.epk, skey);
}

/**
 * Press kits, which belong to their project. The root account holds each one: the public record in its repo and the
 * rest in a space per press kit, whose readers are the project's managers and the people granted access. Requests to
 * see one, and the root's grant records, go into a space per project that only its managers read, so nobody asking or
 * let in sees anyone else's. Managers change a press kit through drafts ({@link EpkDrafts}), and any of them answers
 * requests and grants access, which the root writes as a grant record plus a place in the press kit's space, so access
 * is on the protocol, not in this app's database. Press kits made before the root held them stay in their maker's repo
 * and space until that manager's next visit ({@link LegacyEpks}); until then they're listed and read as before, but
 * only their maker can move them. Every press kit is read through HappyView (lua/computer/localnetwork/epk.lua),
 * strictly where a write or an access decision depends on it, so a HappyView that's down fails those instead of
 * reading as no press kit.
 */
export class EpkService {
	readonly drafts: EpkDrafts;

	private readonly legacy: LegacyEpks;

	private readonly lists = new Map<string, { at: number; value: EpkView[] }>();

	private readonly follows = new Map<string, { at: number; value: Set<string> }>();

	constructor(
		private readonly core: Core,
		private readonly projects: ProjectService,
		/**
		 * Where press kits and managers' Bluesky follows are read (lua/computer/localnetwork/epk.lua and
		 * lua/app/bsky/graph/follow.lua), and their writes waited for.
		 */
		readonly indexed: IndexedRepo,
		private readonly resolveIdentifier: (identifier: string) => Promise<string>,
		private readonly now: () => number = Date.now
	) {
		this.drafts = new EpkDrafts(core, this);
		this.legacy = new LegacyEpks(core, this, projects);
	}

	/** The press kits of a project for its pages, newest first: the root's, then legacy ones not moved yet. */
	async forProject(project: ProjectView): Promise<EpkView[]> {
		return (await this.kits(project)).slice(0, MAX_EPKS_PER_PROJECT);
	}

	/**
	 * Every press kit of a project. A legacy one has moved once the root holds a press kit of the same project under its
	 * key, so the root's wins; one the root holds for another project doesn't count, and the legacy one stays listed.
	 * `strict` for a write that counts them: read fresh, and fail when HappyView can't answer.
	 */
	async kits(project: ProjectView, strict = false): Promise<EpkView[]> {
		const [shared, ...legacy] = await Promise.all([
			this.rootKits(strict),
			...project.managers.map((did) => this.publishedBy(did, strict))
		]);

		const ours = shared.filter((epk) => epk.record.project === project.uri);
		const moved = new Set(ours.map((epk) => epk.rkey));

		return [...ours, ...legacy.flat().filter((epk) => epk.record.project === project.uri && !moved.has(epk.rkey))].sort(
			newestFirst
		);
	}

	/**
	 * A press kit of `project` by its key, for the project's pages and dashboard. Every change to one and every access
	 * decision starts here, so a HappyView that's down fails this rather than reading as a press kit that isn't there.
	 */
	async get(project: ProjectView, rkey: string): Promise<PressKit | null> {
		if (!TID.is(rkey)) return null;
		const shared = await this.rootKit(rkey);

		if (shared?.record.project === project.uri) return { epk: shared, project };

		const legacy = await Promise.all(project.managers.map((did) => this.legacyKit(did, rkey)));
		const epk = legacy.find((view) => view?.record.project === project.uri);

		return epk ? { epk, project } : null;
	}

	/**
	 * A press kit and its project from an old `/epk/<did>/<rkey>` link, which named the repo holding it: the root's
	 * once it moved, else the maker's while the project still lists them as a manager. A root press kit of another
	 * project under the same key isn't the maker's. Null for a hidden project.
	 */
	async locate(did: string, rkey: string): Promise<PressKit | null> {
		if (!TID.is(rkey)) return null;

		const [shared, legacy] = await Promise.all([
			this.rootKit(rkey),
			did === this.core.rootDid ? null : this.legacyKit(did, rkey)
		]);

		const epk = legacy && shared?.record.project !== legacy.record.project ? legacy : shared;

		if (!epk) return null;
		const project = await this.projects.byUri(epk.record.project);

		// Anyone can write a record that names a project; only its managers' legacy press kits count.
		if (!project || project.status === 'hidden' || (epk.legacy && !project.managers.includes(epk.did))) return null;

		return { epk, project };
	}

	/**
	 * A project's press kits for its dashboard, each with the managers' drafts of it, plus the drafts of press kits that
	 * aren't published (new ones, or ones deleted since the draft started).
	 */
	async dashboard(project: ProjectView): Promise<EpkDashboard> {
		const kits = await this.kits(project);
		const drafts = await this.drafts.list(project, kits);

		// A draft of a new press kit belongs to none until it's published, whatever key it has.
		const of = (epk: EpkView) => drafts.filter((draft) => draft.status !== 'new' && draft.rkey === epk.rkey);

		return {
			kits: kits.map((epk) => ({ epk, drafts: of(epk) })),
			drafts: drafts.filter((draft) => !kits.some((epk) => of(epk).includes(draft)))
		};
	}

	/** The private part, read with the root account's membership. */
	async content(epk: EpkView): Promise<EpkContent | null> {
		let stored: StoredRecord | null | undefined;

		try {
			stored = await this.core.readSpaceRecord?.({
				space: epk.space,
				repo: epk.did,
				collection: COLLECTION.epkContent,
				rkey: EPK_CONTENT_KEY
			});
		} catch (error) {
			// The reader refuses a record nested too deep to check its MAC.
			if (!(error instanceof RecordValidationError)) throw error;
			this.core.logger.warn({ space: epk.space, err: error }, 'press kit content does not validate');

			return null;
		}

		if (!stored) return null;

		const content = this.parse(COLLECTION.epkContent, epkContentSchema, stored.value);

		if (!content) this.core.logger.warn({ space: epk.space }, 'press kit content does not validate');

		return content;
	}

	/**
	 * One of a press kit's photos: a public one for anyone, a private one only `withPrivate`. Null for a photo it
	 * doesn't hold, or one the caller can't see.
	 */
	async photo(
		epk: EpkView,
		cid: string,
		withPrivate: boolean
	): Promise<{ bytes: Uint8Array; mimeType: string; isPublic: boolean } | null> {
		if (epk.record.photos?.some((photo) => cidOf(photo) === cid)) {
			const blob = await this.core.fetchBlob?.(epk.did, cid, EPK_PHOTO_MAX_BYTES);

			return blob ? { ...blob, isPublic: true } : null;
		}

		if (!withPrivate || !(await this.content(epk))?.photos?.some((photo) => cidOf(photo) === cid)) return null;
		const blob = await this.core.fetchSpaceBlob?.(epk.space, epk.did, cid);

		return blob ? { ...blob, isPublic: false } : null;
	}

	/** Where `viewer` stands with the private part. Every manager of the project sees all of it. */
	async accessState(kit: PressKit, viewer: Viewer | null): Promise<EpkAccessState> {
		const { epk, project } = kit;

		if (!viewer) {
			const best = ruleFor(epk.record.access, AUDIENCES);

			return best === 'none' ? { kind: 'closed' } : { kind: 'signedOut', rule: best };
		}

		if (project.managers.includes(viewer.did)) return { kind: 'manager' };
		const grant = await this.grantRow(epk.space, viewer.did);

		if (grant) return { kind: 'granted', since: grant.createdAt };

		const latest = await this.core.db
			.selectFrom('epkRequest')
			.select(['status', 'createdAt', 'rule'])
			.where('epkUri', '=', epk.uri)
			.where('requesterDid', '=', viewer.did)
			.orderBy('createdAt', 'desc')
			.executeTakeFirst();

		if (latest?.status === 'pending')
			return { kind: 'pending', since: latest.createdAt, approved: latest.rule === 'auto' };

		if (latest?.status === 'declined') return { kind: 'declined' };
		const rule = await this.ruleForViewer(kit, viewer.did);

		return rule === 'none' ? { kind: 'closed' } : { kind: 'canAsk', rule };
	}

	/** Which audiences `did` is in for this press kit. Everyone signed in is in `anyone`. */
	async audiences(kit: PressKit, did: string): Promise<Set<Audience>> {
		const audiences = new Set<Audience>(['anyone']);

		const threads = await this.core.db
			.selectFrom('bookingThread')
			.select('projectUri')
			.where('promoterDid', '=', did)
			.execute();

		if (threads.length) audiences.add('promoters');

		if (threads.some((thread) => thread.projectUri === kit.project.uri)) audiences.add('bookedWith');

		if (readsFollows(kit.epk.record) && (await this.anyFollows(kit.project.managers, did))) audiences.add('following');

		return audiences;
	}

	async ruleForViewer(kit: PressKit, did: string): Promise<AccessRule> {
		return ruleFor(kit.epk.record.access, await this.audiences(kit, did));
	}

	/**
	 * Deletes a press kit: its space, which takes everyone's access with it, the requests the root wrote for people and
	 * its grants, then the public record.
	 */
	async remove(actor: Viewer, kit: PressKit): Promise<void> {
		await this.assertCanManage(actor, kit);
		const { epk } = kit;
		const root = await this.core.root();

		// The space goes first: a failure after it leaves the record, so deleting again finishes the job.
		if (await root.getSpace(epk.space)) await root.deleteSpace(epk.space);
		await this.dropRecords(epk);
		await root.deleteRecord({ collection: COLLECTION.epk, rkey: epk.rkey });
		await this.indexed.awaitWrite(root.did, COLLECTION.epk, epk.rkey, null);
		await this.forgetIndex(epk.space);
		this.forget();
		this.core.logger.info({ epk: epk.uri, actor: actor.did }, 'deleted a press kit');
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'epk.delete',
			subject: epk.uri,
			subjectKind: 'epk',
			summary: `Deleted the press kit “${epk.record.title}” of ${kit.project.profile.name}`,
			details: { project: kit.project.uri }
		});
	}

	/**
	 * Asks for access. The request goes into the project's request space, which only its managers read, from the asker's
	 * own PDS (or the root's, on their behalf, when theirs can't write to spaces). When the rules approve it at once,
	 * the root writes the grant straight away; for a legacy press kit, only its maker could, so the grant waits for the
	 * press kit to move.
	 */
	async request(actor: Viewer, kit: PressKit, input: { role?: string; message?: string }): Promise<EpkAccessState> {
		const { epk } = kit;
		const state = await this.accessState(kit, actor);

		if (state.kind === 'manager' || state.kind === 'granted') return state;

		if (state.kind === 'pending') throw new UserError('You already asked; the managers will see it');

		if (state.kind === 'declined') throw new UserError("The managers didn't share this press kit with you");

		if (state.kind !== 'canAsk') throw new UserError("This press kit isn't taking requests");
		const role = REQUEST_ROLES.find((value) => value === input.role);
		const message = input.message?.trim();
		const createdAt = nowIso();
		const draft: JsonObject = { epk: epk.uri, createdAt };

		if (role) draft.role = role;

		if (message) draft.message = message;

		const space = await this.ensureRequestSpace(kit.project);
		const { client, onBehalf } = await spaceWriter(this.core, actor);

		if (onBehalf) draft.onBehalfOf = actor.did;
		const record = validateRecord(COLLECTION.epkRequest, draft);
		const ref = await client.createSpaceRecord({ space, collection: COLLECTION.epkRequest, record });

		await this.saveRequest({
			uri: ref.uri,
			space,
			epk: epk.uri,
			requester: actor.did,
			role: role ?? null,
			message: message ?? null,
			rule: state.rule,
			createdAt
		});
		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'epk.request',
				subject: ref.uri,
				subjectKind: 'epk',
				summary: `Asked to see the press kit “${epk.record.title}”`,
				details: { epk: epk.uri, rule: state.rule, onBehalf }
			},
			{ actorClient: onBehalf ? null : client }
		);

		if (state.rule === 'auto' && epk.legacy)
			this.core.logger.info(
				{ epk: epk.uri },
				'auto-approved a legacy press kit request; the grant waits for it to move'
			);
		else if (state.rule === 'auto') await this.grant(kit, { subject: actor.did, via: 'auto', request: ref.uri }, null);

		return this.accessState(kit, actor);
	}

	/** One of the project's managers approves or declines a request. */
	async decide(actor: Viewer, kit: PressKit, requestUri: string, outcome: 'approve' | 'decline'): Promise<void> {
		await this.assertCanManage(actor, kit);
		const { epk } = kit;

		const row = await this.core.db
			.selectFrom('epkRequest')
			.selectAll()
			.where('uri', '=', requestUri)
			.where('epkUri', '=', epk.uri)
			.executeTakeFirst();

		if (!row) throw new NotFoundError('That request does not exist');

		if (row.status !== 'pending') throw new UserError('That request has already been answered');

		if (outcome === 'approve') {
			await this.grant(kit, { subject: row.requesterDid, via: 'approved', request: row.uri }, actor);

			return;
		}

		await this.core.db
			.updateTable('epkRequest')
			.set({ status: 'declined', decidedAt: nowIso() })
			.where('uri', '=', row.uri)
			.execute();
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'epk.decline',
			subject: row.uri,
			subjectKind: 'epk',
			summary: `Declined a request to see the press kit “${epk.record.title}”`
		});
	}

	/** One of the project's managers gives someone access without a request. */
	async invite(actor: Viewer, kit: PressKit, identifier: string): Promise<void> {
		await this.assertCanManage(actor, kit);
		let did: string;

		try {
			did = await this.resolveIdentifier(identifier);
		} catch {
			throw new UserError(`Couldn't find @${identifier.trim().replace(/^@/, '')}`);
		}

		if (kit.project.managers.includes(did)) throw new UserError('Managers of the project already see all of it');
		await this.grant(kit, { subject: did, via: 'invited' }, actor);
	}

	/**
	 * One of the project's managers takes someone's access away: their place in the press kit's space, and the grant
	 * record, wherever it is.
	 */
	async revoke(actor: Viewer, kit: PressKit, subject: string): Promise<void> {
		await this.assertCanManage(actor, kit);
		const { epk } = kit;
		const root = await this.core.root();
		await root.removeMember({ space: epk.space, did: subject });
		const grant = await this.grantRow(epk.space, subject);

		if (grant) await deleteAt(root, grant.uri);
		await this.core.db
			.deleteFrom('epkGrant')
			.where('spaceUri', '=', epk.space)
			.where('subjectDid', '=', subject)
			.execute();
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'epk.revoke',
			subject: epk.uri,
			subjectKind: 'epk',
			summary: `Removed someone's access to the press kit “${epk.record.title}”`,
			details: { subject }
		});
	}

	/** Moves `viewer`'s legacy press kits to the root. Safe to call on every visit: see {@link LegacyEpks.migrate}. */
	migrateLegacy(viewer: Viewer): Promise<LegacyMigration> {
		return this.legacy.migrate(viewer);
	}

	async requests(epk: EpkView): Promise<EpkRequestView[]> {
		const rows = await this.core.db
			.selectFrom('epkRequest')
			.selectAll()
			.where('epkUri', '=', epk.uri)
			.where('status', 'in', ['pending', 'declined'])
			.orderBy('createdAt', 'desc')
			.execute();

		const accounts = await this.core.accounts.views(rows.map((row) => row.requesterDid));

		return rows.map((row) => ({
			uri: row.uri,
			requester: accounts.get(row.requesterDid),
			role: REQUEST_ROLES.find((role): role is RequestRole => role === row.role) ?? null,
			message: row.message,
			status: row.status,
			rule: readRule(row.rule),
			createdAt: row.createdAt,
			decidedAt: row.decidedAt
		}));
	}

	async grants(epk: EpkView): Promise<EpkGrantView[]> {
		const rows = await this.core.db
			.selectFrom('epkGrant')
			.selectAll()
			.where('spaceUri', '=', epk.space)
			.orderBy('createdAt', 'desc')
			.execute();

		const accounts = await this.core.accounts.views(rows.map((row) => row.subjectDid));

		return rows.map((row) => ({
			subject: accounts.get(row.subjectDid),
			via: grantVia(row.via),
			createdAt: row.createdAt
		}));
	}

	/**
	 * Indexes a request synced from a project's request space, or from a press kit's space, where requests went before,
	 * written outside this app.
	 */
	async indexRequest(input: { space: string; uri: string; repoDid: string; value: JsonObject }): Promise<void> {
		const request = epkRequestSchema.safeParse(input.value);

		if (!request.success) return;
		const requester = input.repoDid === this.core.rootDid ? request.data.onBehalfOf : input.repoDid;

		if (!requester) return;
		const kit = await this.requestedKit(input.space, request.data.epk);

		if (!kit || kit.project.managers.includes(requester)) return;
		const granted = await this.grantRow(kit.epk.space, requester);

		await this.saveRequest({
			uri: input.uri,
			space: input.space,
			epk: kit.epk.uri,
			requester,
			role: REQUEST_ROLES.find((value) => value === request.data.role) ?? null,
			message: request.data.message ?? null,
			rule: await this.ruleForViewer(kit, requester),
			createdAt: request.data.createdAt,
			status: granted ? 'approved' : 'pending'
		});
	}

	/**
	 * Indexes a grant synced from a project's request space, for the root press kit it names when that's one of the
	 * project's, or from a press kit's space, where grants went before. Only the space's authority can grant.
	 */
	async indexGrant(input: { space: string; uri: string; repoDid: string; value: JsonObject }): Promise<void> {
		const grant = epkGrantSchema.safeParse(input.value);
		const ref = parseSpaceUri(input.space);

		if (ref?.authority !== input.repoDid || !grant.success) return;
		const { subject, epk, via, request, createdAt } = grant.data;
		let space = input.space;

		if (ref.type === SPACE_TYPE.epkRequests) {
			const kit = epk ? await this.requestedKit(input.space, epk) : null;

			if (!kit || kit.epk.legacy) return;
			space = kit.epk.space;
		} else if (ref.type !== SPACE_TYPE.epk) return;

		await this.saveGrant(space, subject, input.uri, via, request, createdAt);
	}

	async unindexGrant(uri: string): Promise<void> {
		await this.core.db.deleteFrom('epkGrant').where('uri', '=', uri).execute();
	}

	async unindexRequest(uri: string): Promise<void> {
		await this.core.db
			.updateTable('epkRequest')
			.set({ status: 'withdrawn' })
			.where('uri', '=', uri)
			.where('status', '=', 'pending')
			.execute();
	}

	/** The project as the root's repo has it now, when `actor` manages it; access checks shouldn't trust a lagging directory. */
	async assertManager(actor: Viewer, project: ProjectView): Promise<ProjectView> {
		const current = await this.projects.current(project.uri);

		if (!current?.managers.includes(actor.did)) throw new ForbiddenError("Only this project's managers can do that");

		return current;
	}

	/** A press kit the root holds, by its key, as HappyView serves it; a HappyView that's down fails this. */
	async rootKit(rkey: string): Promise<EpkView | null> {
		const stored = await this.indexed.getRecord(this.core.rootDid, COLLECTION.epk, rkey, { strict: true });

		return stored && this.view(this.core.rootDid, stored);
	}

	/**
	 * A press kit's space under the root, created the first time with the project's managers as readers. After that,
	 * ManagerSpaces keeps them in step with the project, and grants add the rest. Only the root writes to it.
	 */
	ensureKitSpace(rkey: string, managers: string[]): Promise<string> {
		return ensureRootSpace(this.core, {
			spaceType: SPACE_TYPE.epk,
			skey: rkey,
			policy: memberSpacePolicy(),
			members: managers,
			access: { read: true, write: false }
		});
	}

	/**
	 * Where people ask to see the project's press kits, and the root keeps its grants: a space under the root that anyone
	 * signed in can write a request into, created the first time with the project's managers as its only readers. The
	 * people granted a press kit are members of that press kit's space, never this one, so they can't read each other's
	 * requests or grants.
	 */
	ensureRequestSpace(project: ProjectView): Promise<string> {
		return ensureRootSpace(this.core, {
			spaceType: SPACE_TYPE.epkRequests,
			skey: project.rkey,
			policy: memberSpacePolicy({ publicWrite: true }),
			members: project.managers,
			access: { read: true, write: false }
		});
	}

	/**
	 * Writes a request indexed somewhere else into the project's request space as the root, on its asker's behalf, for
	 * `epk` (the press kit's URI now), and indexes it with the old one's status. Each asker picked their own request's
	 * key, so two can share one; the root writes each under a key of its own, once: a move that stopped part way finds
	 * the copy it made. Returns the copy's URI.
	 */
	async copyRequest(row: EpkRequestRow, project: ProjectView, epk: string): Promise<string> {
		const space = await this.ensureRequestSpace(project);

		const copied = await this.core.db
			.selectFrom('epkRequest')
			.select('uri')
			.where('spaceUri', '=', space)
			.where('epkUri', '=', epk)
			.where('requesterDid', '=', row.requesterDid)
			.where('createdAt', '=', row.createdAt)
			.executeTakeFirst();

		if (copied) return copied.uri;
		const root = await this.core.root();

		const record = toJson({
			epk,
			role: row.role ?? undefined,
			message: row.message ?? undefined,
			onBehalfOf: row.requesterDid,
			createdAt: row.createdAt
		});

		const ref = await root.createSpaceRecord({
			space,
			collection: COLLECTION.epkRequest,
			record: validateRecord(COLLECTION.epkRequest, record)
		});

		await this.saveRequest({
			uri: ref.uri,
			space,
			epk,
			requester: row.requesterDid,
			role: row.role,
			message: row.message,
			rule: row.rule,
			createdAt: row.createdAt
		});
		// The sync may have indexed the copy first, as a new request.
		await this.core.db
			.updateTable('epkRequest')
			.set({ status: row.status, decidedAt: row.decidedAt })
			.where('uri', '=', ref.uri)
			.execute();

		return ref.uri;
	}

	/**
	 * Writes a grant to a root press kit as the root: the subject as a reader of the press kit's space, then the record
	 * in the project's request space, where only the managers see who else is let in.
	 */
	async writeGrant(
		project: ProjectView,
		epk: Pick<EpkView, 'uri' | 'rkey' | 'space'>,
		grant: GrantInput
	): Promise<void> {
		const root = await this.core.root();
		await root.putMember({ space: epk.space, did: grant.subject, read: true, write: false });

		const ref = await root.putSpaceRecord({
			space: await this.ensureRequestSpace(project),
			collection: COLLECTION.epkGrant,
			rkey: grantKey(epk.rkey, grant.subject),
			record: validateRecord(COLLECTION.epkGrant, toJson({ ...grant, epk: epk.uri }))
		});

		await this.saveGrant(epk.space, grant.subject, ref.uri, grant.via, grant.request, grant.createdAt);
	}

	/**
	 * Uploads a photo to `to`'s repo from where it is now: `from.did`'s public repo, or their repo in `from.space`. A
	 * PDS only accepts a record whose blobs the writer holds, and uploading the same bytes gives the same CID.
	 */
	async copyPhoto(to: RepoClient, photo: EpkPhoto, from: { did: string; space?: string }): Promise<void> {
		const cid = cidOf(photo);

		const blob = from.space
			? await this.core.fetchSpaceBlob?.(from.space, from.did, cid)
			: await this.core.fetchBlob?.(from.did, cid, EPK_PHOTO_MAX_BYTES);

		if (!blob) throw new Error('This app has no way to download photos');
		await to.uploadBlob(blob.bytes, photo.image.mimeType);
	}

	async saveRequest(row: {
		uri: string;
		space: string;
		epk: string;
		requester: string;
		role: string | null;
		message: string | null;
		rule: string;
		createdAt: string;
		status?: 'pending' | 'approved';
	}): Promise<void> {
		await this.core.db
			.insertInto('epkRequest')
			.values({
				uri: row.uri,
				spaceUri: row.space,
				epkUri: row.epk,
				requesterDid: row.requester,
				role: row.role,
				message: row.message,
				rule: row.rule,
				status: row.status ?? 'pending',
				createdAt: row.createdAt,
				decidedAt: null
			})
			.onConflict((oc) => oc.column('uri').doNothing())
			.execute();
	}

	/** Drops a deleted space's requests and grants from the index. */
	async forgetIndex(space: string): Promise<void> {
		await this.core.db.deleteFrom('epkGrant').where('spaceUri', '=', space).execute();
		await this.core.db.deleteFrom('epkRequest').where('epkUri', '=', kitOfSpace(space)).execute();
	}

	/**
	 * The managers of projects with a press kit that reads follows, whose follows HappyView keeps. Strict, since the
	 * re-read that lists them only counts as done once it had them all.
	 */
	async followReaders(): Promise<string[]> {
		const managers = await this.projects.managers();
		const kits = await Promise.all([this.rootKits(true), ...managers.map((did) => this.publishedBy(did, true))]);
		const reading = new Set(kits.flat().flatMap((epk) => (readsFollows(epk.record) ? [epk.record.project] : [])));
		const projects = await this.projects.byUris([...reading]);

		return [...new Set([...projects.values()].flatMap((project) => project.managers))];
	}

	/**
	 * HappyView keeps a manager's follows only while a press kit of one of their projects reads them, so when `published`
	 * is the first of `project`'s that does (`before` lists its press kits until then), it re-reads the follows its
	 * managers made before. Any cached until that's done may be missing those.
	 */
	rereadFollows(project: ProjectView, before: EpkView[], published: Pick<EpkRecord, 'access'>): void {
		if (!readsFollows(published) || before.some((epk) => readsFollows(epk.record))) return;

		for (const did of project.managers)
			void this.indexed.reindex(did, BSKY_FOLLOW, FOLLOWS_REINDEX_MS).then(() => this.follows.delete(did));
	}

	/** Drops the cached lists, after a write changed one. */
	forget(did = this.core.rootDid): void {
		this.lists.delete(did);
	}

	/** A manager's legacy press kits, from their own repo. `strict` as for {@link kits}. */
	async publishedBy(did: string, strict = false): Promise<EpkView[]> {
		return this.listed(did, strict);
	}

	private async grant(kit: PressKit, grant: Omit<GrantInput, 'createdAt'>, actor: Viewer | null): Promise<void> {
		await this.writeGrant(kit.project, kit.epk, { ...grant, createdAt: nowIso() });
		await this.core.audit.record({
			actor: actor?.did ?? this.core.rootDid,
			actorRole: actor ? actorRole(actor) : 'system',
			action: 'epk.grant',
			subject: kit.epk.uri,
			subjectKind: 'epk',
			summary: `${actor ? 'Gave' : 'Automatically gave'} someone access to the press kit “${kit.epk.record.title}”`,
			details: { subject: grant.subject, via: grant.via, request: grant.request },
			via: actor ? undefined : 'system'
		});
	}

	private async saveGrant(
		space: string,
		subject: string,
		uri: string,
		via: string,
		requestUri: string | undefined,
		createdAt: string
	): Promise<void> {
		const request = requestUri ?? null;

		await this.core.db
			.insertInto('epkGrant')
			.values({ spaceUri: space, subjectDid: subject, uri, via, requestUri: request, createdAt })
			.onConflict((oc) => oc.columns(['spaceUri', 'subjectDid']).doUpdateSet({ uri, via, requestUri: request }))
			.execute();
		await this.core.db
			.updateTable('epkRequest')
			.set({ status: 'approved', decidedAt: createdAt })
			.where('epkUri', '=', kitOfSpace(space))
			.where('requesterDid', '=', subject)
			.where('status', '=', 'pending')
			.execute();
	}

	private grantRow(space: string, did: string) {
		return this.core.db
			.selectFrom('epkGrant')
			.selectAll()
			.where('spaceUri', '=', space)
			.where('subjectDid', '=', did)
			.executeTakeFirst();
	}

	/**
	 * The press kit a request in `space` asks for, or null when it can't ask for it there. In a press kit's space, that's
	 * the press kit; in a project's request space, the press kit the request names, when it's one of that project's.
	 */
	private async requestedKit(space: string, epk: string): Promise<PressKit | null> {
		const ref = parseSpaceUri(space);

		if (ref?.type === SPACE_TYPE.epk) {
			const kit = await this.locate(ref.authority, ref.skey);

			return kit?.epk.space === space ? kit : null;
		}

		const named = parsePublicRecordUri(epk);

		if (
			ref?.type !== SPACE_TYPE.epkRequests ||
			ref.authority !== this.core.rootDid ||
			named?.collection !== COLLECTION.epk
		)
			return null;
		const kit = await this.locate(named.did, named.rkey);

		return kit?.project.rkey === ref.skey ? kit : null;
	}

	/**
	 * Deletes the requests for `epk` the root wrote on people's behalf, and its grants. Each row goes with its record, so a
	 * delete that stopped part way picks up where it left off. Deleting a record that's gone, with the press kit's space
	 * or before, does nothing.
	 */
	private async dropRecords(epk: EpkView): Promise<void> {
		const root = await this.core.root();
		const { db } = this.core;
		const requests = await db.selectFrom('epkRequest').select('uri').where('epkUri', '=', epk.uri).execute();

		for (const { uri } of requests) {
			if (parseSpaceRecordUri(uri)?.author !== root.did) continue;
			await deleteAt(root, uri);
			await db.deleteFrom('epkRequest').where('uri', '=', uri).execute();
		}

		const grants = await db.selectFrom('epkGrant').select('uri').where('spaceUri', '=', epk.space).execute();

		for (const { uri } of grants) {
			await deleteAt(root, uri);
			await db.deleteFrom('epkGrant').where('uri', '=', uri).execute();
		}
	}

	private async assertCanManage(actor: Viewer, kit: PressKit): Promise<void> {
		await this.assertManager(actor, kit.project);

		if (kit.epk.legacy) throw new UserError(LEGACY_KIT);
	}

	/** Every press kit the root holds, for every project. */
	private rootKits(strict: boolean): Promise<EpkView[]> {
		return this.listed(this.core.rootDid, strict);
	}

	/**
	 * An account's press kits as HappyView serves them, reused for a minute, except by a `strict` read, which reads them
	 * fresh and fails when HappyView can't answer.
	 */
	private async listed(did: string, strict: boolean): Promise<EpkView[]> {
		const cached = this.lists.get(did);

		if (!strict && cached && this.now() - cached.at < FRESH_MS) return cached.value;
		const records = await this.indexed.listAll(did, COLLECTION.epk, { strict });
		const value = records.flatMap((record) => this.view(did, record) ?? []);

		this.lists.set(did, { at: this.now(), value });

		return value;
	}

	private async legacyKit(did: string, rkey: string): Promise<EpkView | null> {
		const found = await this.indexed.getRecord(did, COLLECTION.epk, rkey, { strict: true });

		return found && this.view(did, found);
	}

	/** A press kit record in `did`'s repo as a view, or null when it doesn't validate. */
	view(did: string, { uri, cid, value }: DirectoryRecord): EpkView | null {
		const rkey = uri.split('/').at(-1)!;
		const record = this.parse(COLLECTION.epk, epkRecordSchema, value);

		if (!record) {
			this.core.logger.debug({ did, rkey }, 'press kit record does not validate');

			return null;
		}

		return {
			uri: `at://${did}/${COLLECTION.epk}/${rkey}`,
			did,
			rkey,
			cid,
			space: epkSpace(did, rkey),
			legacy: did !== this.core.rootDid,
			record
		};
	}

	/** A record checked against its lexicon, then read into its type; null when either fails. */
	private parse<T>(collection: string, schema: z.ZodType<T>, value: JsonObject): T | null {
		try {
			const parsed = schema.safeParse(validateRecord(collection, value));

			return parsed.success ? parsed.data : null;
		} catch (error) {
			if (error instanceof RecordValidationError) return null;
			throw error;
		}
	}

	/** Whether any of `managers` follows `did` on Bluesky. */
	private async anyFollows(managers: string[], did: string): Promise<boolean> {
		for (const manager of managers) if ((await this.followsOf(manager)).has(did)) return true;

		return false;
	}

	/**
	 * Who `did` follows on Bluesky, from the follows HappyView keeps of theirs. A HappyView that's down fails this rather
	 * than caching that they follow no one.
	 */
	private async followsOf(did: string): Promise<Set<string>> {
		const cached = this.follows.get(did);

		if (cached && this.now() - cached.at < FOLLOWS_FRESH_MS) return cached.value;
		const records = await this.indexed.listAll(did, BSKY_FOLLOW, { strict: true, pages: FOLLOW_PAGES });
		const value = new Set(records.flatMap((record) => followSchema.safeParse(record.value).data?.subject ?? []));

		this.follows.set(did, { at: this.now(), value });

		return value;
	}
}
