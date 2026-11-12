import { CREATOR_ROLE } from '#lib/capabilities.ts';
import { profileHref } from '#lib/format.ts';
import { COLLECTION } from '#lib/nsid.ts';
import { performerUri } from '#lib/server/performer-uri.ts';
import {
	actSubject,
	djUnmapped,
	type MigrationBlocker,
	type MigrationPlan,
	type PendingRequest,
	type PlannedPerson,
	type ProjectPlan
} from '#lib/project-migration.ts';
import type { PerformerRecord, ProjectView, RequestRecord } from '#lib/types.ts';
import { backstageSpace, sharedSpace } from '../atproto/spaces';
import type { Core } from '../core';
import { parseJson } from '../db';
import type { RequestTable } from '../db/schema';
import type { EpkService } from './epk';
import type { FollowService } from './follows';
import { ACT_TYPE, DJ_TYPE } from './performer-index';
import type { PerformerService } from './performer-registry';
import type { ProjectService } from './projects';
import type { ShowService } from './shows';

/** The most shows one listShows page holds. */
const SHOWS_PER_PAGE = 100;

/** Roles a claim names that make someone a manager of an act; every other claimant plays in it. */
const MANAGING = new Set(['manager', 'agent']);

type RequestRow = Pick<
	RequestTable,
	'uri' | 'kind' | 'status' | 'requesterDid' | 'projectUri' | 'resultProjectUri' | 'record' | 'createdAt'
>;

const isOpen = (row: RequestRow) => row.status === 'pending' || row.status === 'changes_requested';

const pending = (row: RequestRow): PendingRequest[] =>
	row.kind === 'list' ? [] : [{ uri: row.uri, kind: row.kind, requester: row.requesterDid, createdAt: row.createdAt }];

/**
 * Plans moving every project to a performer: an act in a space of its own, or for a solo project the DJ behind it.
 * It only reads, from the root's repo, HappyView and the app's index, so staff can look before anything moves.
 */
export class ProjectMigration {
	constructor(
		private readonly core: Core,
		private readonly projects: ProjectService,
		private readonly epks: EpkService,
		private readonly shows: ShowService,
		private readonly follows: FollowService,
		private readonly performers: PerformerService
	) {}

	/** `djOverrides` names the DJ, by project URI, for a solo project whose claims and creator don't say. */
	async plan(options: { djOverrides?: Record<string, string> } = {}): Promise<MigrationPlan> {
		// The root's own repo lists every project, whatever the directory has caught up with, as reconcileManagerSpaces does.
		// ast-grep-ignore: no-direct-pds-read
		const records = await (await this.core.root()).listRecords(COLLECTION.project);
		const projects = records.flatMap(({ uri, cid, value }) => this.projects.toView({ uri, cid, record: value }) ?? []);

		const requests: RequestRow[] = await this.core.db
			.selectFrom('request')
			.select(['uri', 'kind', 'status', 'requesterDid', 'projectUri', 'resultProjectUri', 'record', 'createdAt'])
			.orderBy('createdAt')
			.execute();

		const plans = await Promise.all(
			projects.map((project) => this.planProject(project, requests, options.djOverrides?.[project.uri]))
		);

		this.core.logger.info('planned the project migration', { projects: plans.length });

		return {
			projects: plans,
			pendingCreates: requests.filter((row) => row.kind === 'create' && isOpen(row)).flatMap(pending),
			totals: {
				acts: plans.filter((plan) => plan.target === 'act').length,
				djs: plans.filter((plan) => plan.target === 'dj').length,
				blocked: plans.filter((plan) => plan.blockers.length).length
			}
		};
	}

	private async planProject(project: ProjectView, requests: RequestRow[], override?: string): Promise<ProjectPlan> {
		const { rootDid } = this.core;
		const { uri, slug, rkey, profile, managers } = project;
		const space = sharedSpace(rootDid, 'performer', rkey);
		const claims = relationships(project, requests);
		const isDj = profile.kind === 'solo';
		const dj = isDj ? pickDj(project, claims, override) : null;
		const accounts = await this.core.accounts.views([...managers, ...(dj?.did ? [dj.did] : [])]);
		const blockers: MigrationBlocker[] = [];

		const person = (did: string, role: PlannedPerson['role']): PlannedPerson => ({
			did,
			handle: accounts.get(did).handle,
			role,
			relationship: claims.get(did) ?? null
		});

		const people = isDj
			? [
					...(dj?.did ? [person(dj.did, CREATOR_ROLE.performer)] : []),
					...managers.filter((did) => did !== dj?.did).map((did) => person(did, 'manager'))
				]
			: managers.map((did) => person(did, MANAGING.has(claims.get(did) ?? '') ? 'manager' : 'member'));

		if (!managers.length) blockers.push({ kind: 'noManagers' });

		const act = isDj ? null : actSubject(slug, profile);
		const dated = { formerly: [uri], createdAt: project.createdAt, updatedAt: project.updatedAt };
		let record: PerformerRecord | null = null;
		let target: string | null = null;

		if (act) {
			record = { subject: { $type: ACT_TYPE, ...act.subject }, ...dated };
			target = performerUri(rootDid, { kind: 'act', skey: rkey });
			const taken = await this.performers.bySlug(slug);

			if (taken && taken.kind !== 'project' && !taken.formerly.includes(uri))
				blockers.push({ kind: 'slugTaken', slug, space: taken.space });
		} else if (dj?.did) {
			record = { subject: { $type: DJ_TYPE, did: dj.did }, ...dated };
			target = performerUri(rootDid, { kind: 'dj', did: dj.did });
			const taken = await this.performers.forDj(dj.did);

			if (taken && taken.kind !== 'project' && !taken.formerly.includes(uri))
				blockers.push({ kind: 'djSpaceTaken', did: dj.did, space: taken.space });
		} else blockers.push({ kind: 'unresolvedDj' });

		const [details, drafts, kits, threads, follows, upcomingShows] = await Promise.all([
			this.projects.details(uri),
			this.drafts(project),
			this.epks.kits(project, true),
			this.core.db
				.selectFrom('bookingThread')
				.select(['spaceUri as space', 'title', 'status'])
				.where('projectUri', '=', uri)
				.orderBy('createdAt')
				.execute(),
			this.follows.state(project, undefined),
			this.countShows(uri)
		]);

		for (const kit of kits) if (kit.legacy) blockers.push({ kind: 'legacyPressKit', uri: kit.uri, did: kit.did });

		const djAccount = dj?.did ? accounts.get(dj.did) : null;

		return {
			uri,
			slug,
			name: profile.name,
			target: isDj ? 'dj' : 'act',
			dj: dj && { did: dj.did, handle: djAccount?.handle ?? null, rule: dj.rule },
			space,
			performerUri: target,
			record,
			people,
			listed: project.status === 'active',
			hasDetails: !!details && Object.keys(details).length > 0,
			drafts,
			pressKits: kits.flatMap((kit) => (kit.legacy ? [] : [{ uri: kit.uri, title: kit.record.title }])),
			bookingThreads: threads,
			lazy: { follows: follows.followers, upcomingShows },
			decline: requests.filter((row) => row.projectUri === uri && isOpen(row)).flatMap(pending),
			redirect: {
				from: `/projects/${slug}`,
				to: act ? `/act/${slug}` : djAccount ? profileHref(djAccount) : null
			},
			unmapped: act ? act.unmapped : djUnmapped(profile),
			blockers
		};
	}

	/** The current managers' drafts in the project's backstage, which are the ones its dashboard shows. */
	private async drafts(project: ProjectView): Promise<ProjectPlan['drafts']> {
		if (!project.managers.length) return [];

		return this.core.db
			.selectFrom('epkDraft')
			.select(['uri', 'authorDid as author', 'epkRkey', 'updatedAt'])
			.where('spaceUri', '=', backstageSpace(this.core.rootDid, project.rkey))
			.where('authorDid', 'in', project.managers)
			.orderBy('updatedAt', 'desc')
			.execute();
	}

	/** Every upcoming show of the project, page by page. HappyView lists past shows nowhere, so those aren't counted. */
	private async countShows(project: string): Promise<number> {
		let count = 0;

		for (let cursor: string | undefined; ;) {
			const page = await this.shows.upcoming({ project, limit: SHOWS_PER_PAGE, cursor });
			count += page.items.length;

			if (!page.cursor || !page.items.length) return count;
			cursor = page.cursor;
		}
	}
}

/**
 * How each current manager said they're connected, from their approved create or claim request, the latest one winning.
 * A manager staff added directly has none.
 */
function relationships(project: ProjectView, requests: RequestRow[]): Map<string, string> {
	const found = new Map<string, string>();

	for (const row of requests) {
		const isJoin = (row.kind === 'create' || row.kind === 'claim') && row.status === 'approved';

		if (!isJoin || row.resultProjectUri !== project.uri || !project.managers.includes(row.requesterDid)) continue;
		const relationship = parseJson<RequestRecord | null>(row.record, null)?.claim?.relationship;

		if (relationship) found.set(row.requesterDid, relationship);
	}

	return found;
}

/** A solo project's DJ: the first manager who claimed it as themselves, then its creator if they manage it, then staff's pick. */
function pickDj(
	project: ProjectView,
	claims: Map<string, string>,
	override: string | undefined
): Pick<NonNullable<ProjectPlan['dj']>, 'did' | 'rule'> {
	const self = project.managers.find((did) => claims.get(did) === 'self');

	if (self) return { did: self, rule: 'self' };

	if (project.managers.includes(project.createdBy)) return { did: project.createdBy, rule: 'createdBy' };

	return override ? { did: override, rule: 'override' } : { did: null, rule: 'unresolved' };
}
