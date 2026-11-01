import { TID } from '@atproto/common-web';
import type { JsonValue } from '@atproto/lex-json';
import { type LatLng, toE6 } from '#lib/geo.ts';
import { type MatchCriteria, type MatchResult, noteTexts } from '#lib/matching.ts';
import { BSKY_FOLLOW, COLLECTION } from '#lib/nsid.ts';
import { normalizeGenres } from '#lib/taxonomy/genres.ts';
import type {
	FieldChange,
	ProjectDetails,
	ProjectProfile,
	ProjectRecord,
	ProjectStatus,
	ProjectView
} from '#lib/types.ts';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import { type JsonRecord, type RecordRef, xrpcErrorName } from '../atproto/repo-client';
import { parsePublicRecordUri } from '../atproto/spaces';
import { type Core, UserError } from '../core';
import { nowIso, parseJson } from '../db';
import type { Backfills } from '../happyview/backfills';
import { type DirectoryProject, type DirectoryStats, type HappyViewClient, QUERY_BYTES } from '../happyview/client';
import { awaitIndexed, INDEX_WAIT_MS, IndexedRepo, reindex } from '../happyview/indexed';
import { MANAGER_COLLECTIONS } from '../happyview/refresh';
import type { PlyrClient } from '../plyr';
import { truncateUtf8 } from '../utf8';
import { diff } from './diff';
import { ManagerSpaces } from './manager-spaces';
import { withRecordings } from './mix-recordings';
import { SetService } from './sets';

export type ProjectSearch = {
	q?: string;
	near?: LatLng;
	radiusKm?: number;
	genres?: string[];
	kinds?: string[];
	formats?: string[];
	setTypes?: string[];
	feeTiers?: string[];
	country?: string;
	/** Only projects this account manages. */
	manager?: string;
	includeInactive?: boolean;
	/** Restrict to one status (implies includeInactive). */
	status?: ProjectStatus;
	/** Only projects nobody has claimed yet. */
	unclaimed?: boolean;
	sort?: 'relevance' | 'distance' | 'recent' | 'name';
	page?: number;
	pageSize?: number;
};

export type ProjectSearchResult = { items: ProjectView[]; total: number; page: number; pageSize: number };

/**
 * The page asked for, and the directory params every performer search takes (searchProjects and searchPerformers):
 * words, place, the filters, sort, and paging.
 */
export function searchQuery(params: ProjectSearch) {
	const page = Math.max(1, params.page ?? 1);
	const pageSize = Math.min(60, Math.max(1, params.pageSize ?? 24));

	const query = {
		q: params.q && truncateUtf8(params.q, QUERY_BYTES),
		latE6: params.near && toE6(params.near.lat),
		lngE6: params.near && toE6(params.near.lng),
		radiusKm: params.radiusKm && Math.round(params.radiusKm),
		// The directory normalizes genres itself. Doing it here keeps the list within the lexicon's limits.
		genres: params.genres && normalizeGenres(params.genres, 20),
		kinds: params.kinds,
		formats: params.formats,
		setTypes: params.setTypes,
		feeTiers: params.feeTiers,
		country: params.country,
		unclaimed: params.unclaimed,
		sort: params.sort,
		limit: pageSize,
		cursor: page > 1 ? String((page - 1) * pageSize) : undefined
	};

	return { page, pageSize, query };
}

const PROJECT_STATUSES: readonly ProjectStatus[] = ['active', 'hidden', 'archived'];

export const isProjectStatus = (value: JsonValue | undefined): value is ProjectStatus =>
	PROJECT_STATUSES.some((status) => status === value);

/**
 * Checks a project record the directory served against the project lexicon.
 * The lexicon leaves `status` open, so an unknown one is rejected here too.
 */
function assertProjectRecord(value: JsonRecord): asserts value is JsonRecord & ProjectRecord {
	validateRecord(COLLECTION.project, value);

	if (!isProjectStatus(value.status)) throw new RecordValidationError(COLLECTION.project, 'unknown status');
}

export function slugify(input: string): string {
	const slug = input
		.normalize('NFKD')
		.replace(/[̀-ͯ]/g, '')
		.toLowerCase()
		.replace(/&/g, ' and ')
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, 48)
		.replace(/-+$/g, '');

	return slug.length >= 2 ? slug : `project-${slug || 'x'}`;
}

/** The most URIs one getProjects call takes (its lexicon's maxLength). */
const URIS_PER_LOOKUP = 25;

/** What slugify produces, so a route param that can't be a slug skips the lookup. */
const SLUG = /^[a-z0-9-]{1,64}$/;

function projectView(rootDid: string, ref: RecordRef & { rkey: string }, record: ProjectRecord): ProjectView {
	const avatar = record.profile.avatar?.ref.$link;

	return {
		uri: ref.uri,
		rkey: ref.rkey,
		cid: ref.cid,
		slug: record.slug,
		profile: record.profile,
		managers: record.managers,
		status: record.status,
		revision: record.revision,
		createdBy: record.createdBy,
		createdAt: record.createdAt,
		updatedAt: record.updatedAt,
		avatarUrl: avatar ? `/media/${rootDid}/${avatar}` : null
	};
}

/**
 * Published projects. They live in the root account's public repo and HappyView indexes them, so
 * reads go to the directory queries it serves; private details stay in the industry space.
 */
export class ProjectService {
	/** The DJ sets mixes link, which `publish` checks. */
	readonly sets: SetService;
	/** The spaces whose members follow each project's managers, which `publish` updates. */
	private readonly managerSpaces: ManagerSpaces;

	/**
	 * `backfills` is the app's, which other re-reads of the same accounts share (SetService makes one without it).
	 * `plyr` finds the records of plyr.fm tracks that mixes link; without it, only recordings already published stay.
	 */
	constructor(
		private readonly core: Core,
		private readonly directory: HappyViewClient,
		private readonly indexWaitMs = INDEX_WAIT_MS,
		backfills?: Backfills,
		private readonly plyr?: PlyrClient
	) {
		this.sets = new SetService(core, directory, backfills);
		this.managerSpaces = new ManagerSpaces(core, new IndexedRepo(directory, core.logger, indexWaitMs));
	}

	/**
	 * Brings every project's spaces in line with its managers (run at startup and every hour), finishing any sync that
	 * failed part way. It reads the projects from the root's own repo, since a directory that's behind would undo a
	 * recent change.
	 */
	async reconcileManagerSpaces(): Promise<void> {
		// The root's own repo decides who's in a thread: a directory that's behind could let a removed manager back in.
		// ast-grep-ignore: no-direct-pds-read
		const records = await (await this.core.root()).listRecords(COLLECTION.project);

		for (const { uri, cid, value } of records) {
			const project = this.toView({ uri, cid, record: value });

			if (project) await this.managerSpaces.sync(project.uri, project.managers);
		}
	}

	async indexDetails(projectUri: string, uri: string, cid: string, details: ProjectDetails, updatedAt: string) {
		const json = JSON.stringify(details);
		await this.core.db
			.insertInto('projectDetails')
			.values({ projectUri, uri, cid, details: json, updatedAt })
			.onConflict((oc) => oc.column('projectUri').doUpdateSet({ uri, cid, details: json, updatedAt }))
			.execute();
	}

	async details(projectUri: string): Promise<ProjectDetails | null> {
		const row = await this.core.db
			.selectFrom('projectDetails')
			.select('details')
			.where('projectUri', '=', projectUri)
			.executeTakeFirst();

		return row ? parseJson<ProjectDetails>(row.details, {}) : null;
	}

	async bySlug(slug: string): Promise<ProjectView | null> {
		if (!SLUG.test(slug)) return null;
		const { project } = await this.directory.query('getProject', { slug });

		return project ? this.toView(project) : null;
	}

	async byUri(uri: string): Promise<ProjectView | null> {
		if (!this.projectRkey(uri)) return null;
		const { project } = await this.directory.query('getProject', { uri });

		return project ? this.toView(project) : null;
	}

	/** A project as the root's own repo has it now, for a read that a directory still indexing a change would get wrong. */
	async current(uri: string): Promise<ProjectView | null> {
		const rkey = this.projectRkey(uri);

		if (!rkey) return null;
		// The root's own repo decides who's in a new thread, as in reconcileManagerSpaces.
		// ast-grep-ignore: no-direct-pds-read
		const stored = await (await this.core.root()).getRecord({ collection: COLLECTION.project, rkey });

		return stored && this.toView({ uri: stored.uri, cid: stored.cid, record: stored.value });
	}

	async byUris(uris: string[]): Promise<Map<string, ProjectView>> {
		const wanted = [...new Set(uris)].filter((uri) => this.projectRkey(uri));
		const lookups = [];

		for (let start = 0; start < wanted.length; start += URIS_PER_LOOKUP) {
			lookups.push(this.directory.query('getProjects', { uris: wanted.slice(start, start + URIS_PER_LOOKUP) }));
		}

		const views = this.toViews((await Promise.all(lookups)).flatMap((lookup) => lookup.projects));

		return new Map(views.map((view) => [view.uri, view]));
	}

	/** Every project the account manages, in any status, by name. */
	managedBy(did: string): Promise<ProjectView[]> {
		return this.everyPage({ manager: did, includeInactive: true, sort: 'name' });
	}

	/** Every account that manages a project, in any status. */
	async managers(): Promise<string[]> {
		const projects = await this.everyPage({ includeInactive: true, sort: 'name' });

		return [...new Set(projects.flatMap((project) => project.managers))];
	}

	/** Every project a search finds, page by page. */
	private async everyPage(params: Omit<ProjectSearch, 'page' | 'pageSize'>): Promise<ProjectView[]> {
		const items: ProjectView[] = [];

		for (let page = 1; ; page++) {
			const result = await this.search({ ...params, page, pageSize: 60 });
			items.push(...result.items);

			if (page * result.pageSize >= result.total) return items;
		}
	}

	async recent(limit = 8): Promise<ProjectView[]> {
		return (await this.search({ sort: 'recent', pageSize: limit })).items;
	}

	/** Directory totals, plus the genres and cities of active projects, most common first. */
	stats(): Promise<DirectoryStats> {
		return this.directory.query('getStats');
	}

	async search(params: ProjectSearch): Promise<ProjectSearchResult> {
		const { page, pageSize, query } = searchQuery(params);

		const output = await this.directory.query('searchProjects', {
			...query,
			manager: params.manager,
			status: params.status,
			includeInactive: params.includeInactive
		});

		return { items: this.toViews(output.projects), total: output.total, page, pageSize };
	}

	async match(criteria: MatchCriteria, limit = 30): Promise<MatchResult[]> {
		const { matches } = await this.directory.query('matchProjects', {
			latE6: criteria.location && toE6(criteria.location.lat),
			lngE6: criteria.location && toE6(criteria.location.lng),
			radiusKm: Math.round(criteria.radiusKm),
			genres: normalizeGenres(criteria.genres),
			budgetTier: criteria.budgetTier,
			capacity: criteria.capacity,
			setType: criteria.setType,
			formats: criteria.formats,
			limit
		});

		return matches.flatMap((match) => {
			const candidate = this.toView(match.project);

			if (!candidate) return [];

			return [
				{
					candidate,
					score: match.score,
					breakdown: match.breakdown,
					reasons: noteTexts(match.reasons),
					warnings: noteTexts(match.warnings)
				}
			];
		});
	}

	async uniqueSlug(name: string): Promise<string> {
		const base = slugify(name);

		for (let n = 1; n < 1000; n++) {
			const candidate = n === 1 ? base : `${base}-${n}`;
			const { project } = await this.directory.query('getProject', { slug: candidate });

			if (!project) return candidate;
		}

		return `${base}-${TID.nextStr()}`;
	}

	/**
	 * Writes a project to the root account's public repo (compare-and-swap on
	 * the previous CID) once the sets its mixes newly link check out and its
	 * plyr.fm mixes point at their tracks' records (see withRecordings), moves its
	 * spaces' members along when its managers change, optionally updates its
	 * private details in the industry space, and waits for the directory to
	 * serve it. Returns field-level changes.
	 */
	async publish(input: {
		existing?: ProjectView | null;
		profile: ProjectProfile;
		managers: string[];
		status: ProjectStatus;
		createdBy: string;
		details?: ProjectDetails | null;
		lastRequest?: string;
	}): Promise<{ project: ProjectView; changes: FieldChange[] }> {
		const { existing } = input;
		const root = await this.core.root();
		const now = nowIso();
		const rkey = existing?.rkey ?? TID.nextStr();
		const managers = [...new Set(input.managers)].slice(0, 10);
		await this.sets.assertLinkable(input.profile, existing?.profile, managers);
		const profile = await withRecordings(input.profile, existing?.profile, managers, this.plyr);

		const record: ProjectRecord = {
			slug: existing?.slug ?? (await this.uniqueSlug(profile.name)),
			profile,
			managers,
			status: input.status,
			revision: (existing?.revision ?? 0) + 1,
			createdBy: existing?.createdBy ?? input.createdBy,
			createdAt: existing?.createdAt ?? now,
			updatedAt: now
		};

		if (input.lastRequest) record.lastRequest = input.lastRequest;
		const validated = validateRecord(COLLECTION.project, record);
		let ref: RecordRef;

		try {
			ref = await root.putRecord({
				collection: COLLECTION.project,
				rkey,
				record: validated,
				swapRecord: existing?.cid ?? null
			});
		} catch (error) {
			if (xrpcErrorName(error) === 'InvalidSwap') {
				throw new UserError('This project changed while you were editing. Reload and try again.', 409);
			}

			throw error;
		}

		const changes = diff(
			existing ? { profile: existing.profile, managers: existing.managers, status: existing.status } : {},
			{ profile: record.profile, managers: record.managers, status: record.status }
		);

		if (existing && changes.some((change) => change.field === 'managers'))
			await this.managerSpaces.sync(ref.uri, managers);

		if (input.details !== undefined) {
			const before = existing ? await this.details(existing.uri) : null;
			const details = input.details ?? {};
			const detailRecord = validateRecord(COLLECTION.projectDetails, { project: ref.uri, details, updatedAt: now });

			const written = await root.putSpaceRecord({
				space: this.core.spaces.industry,
				collection: COLLECTION.projectDetails,
				rkey,
				record: detailRecord
			});

			await this.indexDetails(ref.uri, written.uri, written.cid, details, now);
			changes.push(
				...diff(before ?? {}, details, 'details').map((change) => ({
					...change,
					before: change.before ? '[private]' : undefined,
					after: change.after ? '[private]' : undefined
				}))
			);
		}

		const indexed = await this.awaitIndexed(ref);
		const added = managers.filter((did) => !existing?.managers.includes(did));

		// A new manager's older records are read in the background, once HappyView serves the project that lets their
		// press kits in; until then that re-read would be turned away, so the set picker reads their sets later. Sets go
		// through the picker's own re-reads, which it waits for. Their Bluesky follows are re-read too, which HappyView
		// keeps when one of the project's press kits reads follows.
		if (indexed) {
			void this.sets.refresh(added);

			for (const collection of [...MANAGER_COLLECTIONS.filter((nsid) => nsid !== COLLECTION.djSet), BSKY_FOLLOW])
				for (const did of added) void reindex(this.directory, this.core.logger, collection, did);
		}

		return { project: projectView(this.core.rootDid, { ...ref, rkey }, record), changes };
	}

	/** Waits for the directory to serve `ref`, so the next page load reads this write, and says whether it did. */
	private async awaitIndexed(ref: RecordRef): Promise<boolean> {
		const served = async () => (await this.directory.query('getProject', { uri: ref.uri })).project?.cid === ref.cid;
		const indexed = await awaitIndexed(served, this.indexWaitMs);

		if (!indexed)
			this.core.logger.warn({ uri: ref.uri, cid: ref.cid }, 'the directory has not caught up with a project write yet');

		return indexed;
	}

	/** The rkey of one of the root account's project records, or null for any other URI. */
	private projectRkey(uri: string): string | null {
		const parsed = parsePublicRecordUri(uri);

		return parsed?.did === this.core.rootDid && parsed.collection === COLLECTION.project ? parsed.rkey : null;
	}

	private toViews(projects: DirectoryProject[]): ProjectView[] {
		return projects.flatMap((project) => this.toView(project) ?? []);
	}

	/** The app's view of a directory project, or null (logged) if it isn't a valid project the root published. */
	toView({ uri, cid, record, distanceM }: DirectoryProject): ProjectView | null {
		const rkey = this.projectRkey(uri);

		if (!rkey) {
			this.core.logger.warn({ uri }, 'skipping a directory project the root account did not publish');

			return null;
		}

		try {
			assertProjectRecord(record);
		} catch (error) {
			if (!(error instanceof RecordValidationError)) throw error;
			this.core.logger.warn({ err: error, uri }, 'skipping an invalid project record');

			return null;
		}

		const view = projectView(this.core.rootDid, { uri, cid, rkey }, record);

		if (distanceM !== undefined) view.distanceKm = distanceM / 1000;

		return view;
	}
}
