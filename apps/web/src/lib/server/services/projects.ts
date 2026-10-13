import { setTimeout as sleep } from 'node:timers/promises';
import { TID } from '@atproto/common-web';
import type { JsonValue } from '@atproto/lex-json';
import { type LatLng, toE6 } from '#lib/geo.ts';
import { type MatchCriteria, type MatchResult, noteTexts } from '#lib/matching.ts';
import { COLLECTION } from '#lib/nsid.ts';
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
import type { DirectoryProject, DirectoryStats, HappyViewClient } from '../happyview/client';
import { diff } from './diff';

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

/** How long `publish` waits for the directory to serve a write before answering without it. */
const INDEX_WAIT_MS = 5_000;

const INDEX_POLL_MS = 100;

/** The most URIs one getProjects call takes (its lexicon's maxLength). */
const URIS_PER_LOOKUP = 25;

/** The longest search text the directory takes, in UTF-8 bytes (its lexicon's maxLength). */
const QUERY_BYTES = 100;

const utf8 = new TextEncoder();

/** `text` cut to whole characters that fit in `bytes` UTF-8 bytes. */
function truncateUtf8(text: string, bytes: number): string {
	let out = '';

	for (const char of text) {
		if (utf8.encode(out + char).length > bytes) break;
		out += char;
	}

	return out;
}

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
	constructor(
		private readonly core: Core,
		private readonly directory: HappyViewClient,
		private readonly indexWaitMs = INDEX_WAIT_MS
	) {}

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
	async managedBy(did: string): Promise<ProjectView[]> {
		const items: ProjectView[] = [];

		for (let page = 1; ; page++) {
			const result = await this.search({ manager: did, includeInactive: true, sort: 'name', page, pageSize: 60 });
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
		const page = Math.max(1, params.page ?? 1);
		const pageSize = Math.min(60, Math.max(1, params.pageSize ?? 24));

		const output = await this.directory.query('searchProjects', {
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
			manager: params.manager,
			status: params.status,
			includeInactive: params.includeInactive,
			unclaimed: params.unclaimed,
			sort: params.sort,
			limit: pageSize,
			cursor: page > 1 ? String((page - 1) * pageSize) : undefined
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
	 * the previous CID), optionally updates its private details in the
	 * industry space, and waits for the directory to serve it. Returns
	 * field-level changes.
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

		const record: ProjectRecord = {
			slug: existing?.slug ?? (await this.uniqueSlug(input.profile.name)),
			profile: input.profile,
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

		await this.awaitIndexed(ref);

		return { project: projectView(this.core.rootDid, { ...ref, rkey }, record), changes };
	}

	/**
	 * Polls the directory until it serves `ref`, so the next page load reads this write. Gives up
	 * after a few seconds rather than failing: the write happened, and the directory catches up on its own.
	 */
	private async awaitIndexed(ref: RecordRef): Promise<void> {
		const deadline = Date.now() + this.indexWaitMs;

		while (Date.now() < deadline) {
			const served = await this.directory.query('getProject', { uri: ref.uri }).catch(() => null);

			if (served?.project?.cid === ref.cid) return;
			await sleep(INDEX_POLL_MS);
		}

		this.core.logger.warn({ uri: ref.uri, cid: ref.cid }, 'the directory has not caught up with a project write yet');
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
	private toView({ uri, cid, record, distanceM }: DirectoryProject): ProjectView | null {
		const rkey = this.projectRkey(uri);

		if (!rkey) {
			this.core.logger.warn({ uri }, 'skipping a directory project the root account did not publish');

			return null;
		}

		try {
			assertProjectRecord(record);
		} catch (error) {
			if (!(error instanceof RecordValidationError)) throw error;
			this.core.logger.warn({ err: error, uri }, 'skipping an invalid project record from the directory');

			return null;
		}

		const view = projectView(this.core.rootDid, { uri, cid, rkey }, record);

		if (distanceM !== undefined) view.distanceKm = distanceM / 1000;

		return view;
	}
}
