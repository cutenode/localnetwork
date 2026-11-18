import { setTimeout as sleep } from 'node:timers/promises';
import type { JsonObject } from '@atproto/lex-json';
import type { LexiconDoc } from '@atproto/lexicon';
import { z } from 'zod';
import type { MatchBreakdown, MatchNote } from '#lib/matching.ts';
import { DIRECTORY } from '#lib/nsid.ts';
import type { Charts, DirectoryDj, DirectoryPlace, DjSetView, SupportedArtist, TrackPlays } from '#lib/types.ts';
import { lexicons } from '../atproto/lexicons';
import type { Config } from '../config';
import type { HappyViewScript } from './bundle';

/** The longest search text the directory's queries take, in UTF-8 bytes (their lexicons' maxLength). */
export const QUERY_BYTES = 100;

/** A published project as the directory serves it (`computer.localnetwork.directory.defs#projectView`). */
export type DirectoryProject = { uri: string; cid: string; record: JsonObject; distanceM?: number };

/** A listed DJ's profile as the directory serves it (`computer.localnetwork.directory.defs#profileView`). */
export type DirectoryProfile = {
	uri: string;
	cid: string;
	record: JsonObject;
	/** The root's listing of it. */
	listing: string;
	/** What it was searched by: its performer section's genres, or the ones the DJ plays most. */
	genres: string[];
	distanceM?: number;
};

/** A performer the directory found (`computer.localnetwork.directory.defs#performerView`): a project or a DJ's profile. */
export type DirectoryPerformer = { project: DirectoryProject } | { profile: DirectoryProfile };

/** A record as the directory indexed it (`computer.localnetwork.directory.defs#recordView`), as its record script kept it. */
export type DirectoryRecord = { uri: string; cid: string; value: JsonObject };

type Facet = { value: string; count: number };

/** A show as the directory serves it (`computer.localnetwork.directory.defs#showView`). */
export type DirectoryShow = {
	uri: string;
	cid: string;
	record: JsonObject;
	project: DirectoryProject;
	distanceM?: number;
};

export type DirectoryMatch = {
	project: DirectoryProject;
	score: number;
	inRange: boolean;
	breakdown: MatchBreakdown;
	reasons: MatchNote[];
	warnings: MatchNote[];
};

/** What each directory query answers, as its lexicon in lexicons/computer/localnetwork/directory describes it. */
export type DirectoryOutputs = {
	searchProjects: { projects: DirectoryProject[]; total: number; cursor?: string };
	searchPerformers: { performers: DirectoryPerformer[]; total: number; cursor?: string };
	matchProjects: { matches: DirectoryMatch[] };
	getProject: { project?: DirectoryProject; pressKits?: number; upcomingShows?: number; djs?: number };
	getProjects: { projects: DirectoryProject[] };
	getStats: {
		total: number;
		active: number;
		unclaimed: number;
		cityCount: number;
		genres: Facet[];
		cities: DirectoryPlace[];
	};
	getSets: { sets: DjSetView[] };
	listSets: { sets: DjSetView[] };
	listShows: { shows: DirectoryShow[]; cursor?: string };
	getShows: { shows: DirectoryShow[] };
	getFollowStatus: { followers: number; follows: string[] };
	getCharts: Charts;
	getArtist: { artist?: SupportedArtist; tracks: TrackPlays[]; djs: DirectoryDj[] };
	searchArtists: { artists: SupportedArtist[] };
	getRecords: { records: DirectoryRecord[] };
	listRecords: { records: DirectoryRecord[]; cursor?: string };
};

export type DirectoryStats = DirectoryOutputs['getStats'];

/** Query params before encoding. Arrays become repeated keys, the way XRPC sends them. */
type QueryParams = Record<string, string | number | boolean | string[] | undefined>;

const storedLexicon = z.object({ lexicon_json: z.json() });

const storedScripts = z.array(z.object({ id: z.string(), body: z.string() }));

const backfillJob = z.object({ id: z.string(), status: z.string() });

const BACKFILL_POLL_MS = 250;

/** The most accounts one backfill job takes (`dids` in POST /admin/backfill). */
const BACKFILL_DIDS = 500;

/**
 * Backfill statuses that end a job without all its work done. HappyView 3 pauses a job that ran out of retries with
 * repos still queued, until someone resumes it.
 */
export const BACKFILL_FAILED = new Set(['failed', 'cancelled', 'paused']);

const BACKFILL_DONE = new Set(['completed', ...BACKFILL_FAILED]);

/** How far behind HappyView's Jetstream consumer is (GET /admin/jetstream/status). */
export type JetstreamStatus = { connected: boolean; lagMs: number | null };

/** How one script trigger ran over a window: runs, the ones that ended any way but `ok`, and p95 wall time. */
export type ScriptHealth = { trigger: string; runs: number; failures: number; p95Ms: number };

/** Script health over a window (GET /admin/metrics/scripts), busiest trigger first. Empty while metrics are off. */
export type ScriptMetrics = { enabled: boolean; scripts: ScriptHealth[] };

const jetstreamStatus = z
	.object({ connected: z.boolean(), lag_ms: z.number().nullable() })
	.transform(({ connected, lag_ms }): JetstreamStatus => ({ connected, lagMs: lag_ms }));

const deadLetterCount = z.object({ count: z.number().int().nonnegative() });

const scriptMetrics = z
	.object({
		enabled: z.boolean(),
		scripts: z
			.array(
				z.object({
					trigger: z.string(),
					runs: z.number().int().nonnegative(),
					// `ok`, `trap`, `no_interpreter`, `error`, timeouts and instruction limits: anything but `ok` failed.
					outcomes: z.record(z.string(), z.number().int().nonnegative()),
					wall_ms: z.object({ p95: z.number() })
				})
			)
			.default([])
	})
	.transform(({ enabled, scripts }): ScriptMetrics => ({
		enabled,
		scripts: scripts
			.map(({ trigger, runs, outcomes, wall_ms }) => ({
				trigger,
				runs,
				failures: Object.entries(outcomes).reduce((sum, [outcome, count]) => sum + (outcome === 'ok' ? 0 : count), 0),
				p95Ms: wall_ms.p95
			}))
			.toSorted((a, b) => b.runs - a.runs)
	}));

/**
 * The admin permissions `syncHappyView`, the re-reads (Backfills, reindex) and the System page's health numbers use, so
 * HAPPYVIEW_API_KEY needs exactly these. @internal
 */
export const HAPPYVIEW_PERMISSIONS = [
	'lexicons:read',
	'lexicons:create',
	'scripts:read',
	'scripts:manage',
	'backfill:create',
	'backfill:read',
	'stats:read',
	'dead-letters:read',
	'metrics:read'
] as const;

const TIMEOUT_MS = 10_000;

export class HappyViewError extends Error {
	constructor(
		readonly status: number,
		message: string
	) {
		super(message);
		this.name = 'HappyViewError';
	}
}

/**
 * Talks to the HappyView instance that indexes published projects, their managers' DJ records, press kits and shows,
 * follows, and the Bluesky and userinput.app records the site reads: directory queries over XRPC, plus the admin calls
 * that keep its lexicons and scripts in step with this app.
 */
export class HappyViewClient {
	readonly url: string;
	private readonly apiKey?: string;
	/** The API client directory queries identify themselves as. A server proves it with the secret. */
	private readonly client: Config['happyView']['client'];
	private readonly fetch: typeof globalThis.fetch;

	constructor(options: Config['happyView'] & { fetch?: typeof globalThis.fetch }) {
		this.url = options.url;
		this.apiKey = options.apiKey;
		this.client = options.client;
		this.fetch = options.fetch ?? globalThis.fetch;
	}

	/** Runs a directory query. Params and output are checked against the query's lexicon. */
	async query<K extends keyof DirectoryOutputs>(name: K, params: QueryParams = {}): Promise<DirectoryOutputs[K]> {
		const nsid = DIRECTORY[name];
		lexicons.assertValidXrpcParams(nsid, params);

		// Percent-encoded by hand: HappyView doesn't read `+` as a space, which URLSearchParams would write.
		const query = Object.entries(params).flatMap(([key, value]) =>
			[value ?? []].flat().map((item) => `${key}=${encodeURIComponent(item)}`)
		);

		const response = await this.request(`/xrpc/${nsid}?${query.join('&')}`);

		// SAFETY: assertValidXrpcOutput checked the body against the query's lexicon, which DirectoryOutputs mirrors.
		return lexicons.assertValidXrpcOutput(nsid, await response.json()) as DirectoryOutputs[K];
	}

	/** Whether this client can make admin calls, which need HAPPYVIEW_API_KEY. */
	get admin(): boolean {
		return this.apiKey !== undefined;
	}

	/** Whether HappyView answers at all, within a couple of seconds. */
	async health(): Promise<boolean> {
		const response = await this.fetch(`${this.url}/health`, { signal: AbortSignal.timeout(2_000) }).catch(() => null);

		return response?.ok ?? false;
	}

	/** The lexicon document HappyView has stored under `id`, or null when it has none. */
	async lexicon(id: string) {
		try {
			const response = await this.request(`/admin/lexicons/${id}`, { admin: true });

			return storedLexicon.parse(await response.json()).lexicon_json;
		} catch (error) {
			if (error instanceof HappyViewError && error.status === 404) return null;

			throw error;
		}
	}

	async putLexicon(doc: LexiconDoc): Promise<void> {
		// No backfill on upload: HappyView would crawl every repo on the network, and only the root's records count.
		await this.request('/admin/lexicons', { admin: true, body: { lexicon_json: doc, backfill: false } });
	}

	async scripts(): Promise<HappyViewScript[]> {
		const response = await this.request('/admin/scripts', { admin: true });

		return storedScripts.parse(await response.json());
	}

	async putScript(script: HappyViewScript): Promise<void> {
		await this.request('/admin/scripts', { admin: true, body: { ...script, script_type: 'lua' } });
	}

	/**
	 * Has HappyView re-read the records of `collection` in each repo `dids` lists from its PDS, through the record script.
	 * HappyView takes 500 accounts per job, so a longer list starts a job per 500, one after another. Returns the job ids.
	 */
	async backfill(collection: string, dids: string[]): Promise<string[]> {
		const jobs: string[] = [];

		for (let start = 0; start < dids.length; start += BACKFILL_DIDS) {
			const body = { collection, dids: dids.slice(start, start + BACKFILL_DIDS) };
			const response = await this.request('/admin/backfill', { admin: true, body });
			jobs.push(backfillJob.parse(await response.json()).id);
		}

		return jobs;
	}

	/** A backfill job's status (`running`, `completed`, `failed`, ...), or null when HappyView doesn't know it. */
	async backfillStatus(id: string): Promise<string | null> {
		const response = await this.request('/admin/backfill/status', { admin: true });

		return (
			z
				.array(backfillJob)
				.parse(await response.json())
				.find((job) => job.id === id)?.status ?? null
		);
	}

	/**
	 * Polls a backfill job until it's done or `waitMs` passes, and returns its last status (null when HappyView
	 * doesn't know the job). A job still running at the deadline carries on in HappyView.
	 */
	async awaitBackfill(id: string, waitMs: number): Promise<string | null> {
		const deadline = Date.now() + waitMs;
		let status = await this.backfillStatus(id);

		while (status && !BACKFILL_DONE.has(status) && Date.now() < deadline) {
			await sleep(BACKFILL_POLL_MS);
			status = await this.backfillStatus(id);
		}

		return status;
	}

	/** Whether HappyView's Jetstream consumer is connected, and how far behind it is (`stats:read`). */
	async jetstreamStatus(): Promise<JetstreamStatus> {
		const response = await this.request('/admin/jetstream/status', { admin: true });

		return jetstreamStatus.parse(await response.json());
	}

	/** How many events HappyView couldn't index and set aside (`dead-letters:read`). */
	async deadLetterCount(): Promise<number> {
		const response = await this.request('/admin/dead-letters/count', { admin: true });

		return deadLetterCount.parse(await response.json()).count;
	}

	/** How each script trigger ran over the last `period` (`24h`, ...), busiest first (`metrics:read`). */
	async scriptMetrics(period: string): Promise<ScriptMetrics> {
		const response = await this.request(`/admin/metrics/scripts?window=${encodeURIComponent(period)}`, { admin: true });

		return scriptMetrics.parse(await response.json());
	}

	private async request(path: string, options: { admin?: boolean; body?: object } = {}) {
		const url = new URL(`${this.url}${path}`);
		const headers = new Headers();

		if (options.admin) {
			if (!this.apiKey) throw new Error('HappyView admin calls need HAPPYVIEW_API_KEY');
			headers.set('authorization', `Bearer ${this.apiKey}`);
		} else if (this.client) {
			headers.set('x-client-key', this.client.key);
			headers.set('x-client-secret', this.client.secret);
		}

		if (options.body) headers.set('content-type', 'application/json');

		let response: Response;

		try {
			response = await this.fetch(url, {
				method: options.body ? 'POST' : 'GET',
				headers,
				body: options.body && JSON.stringify(options.body),
				signal: AbortSignal.timeout(TIMEOUT_MS)
			});
		} catch (cause) {
			// Unreachable or too slow reads the same to callers as HappyView saying it's unavailable.
			throw new HappyViewError(
				503,
				`HappyView ${url.pathname}: ${cause instanceof Error ? cause.message : String(cause)}`
			);
		}

		if (!response.ok) {
			const detail = (await response.text()).slice(0, 300);

			throw new HappyViewError(response.status, `HappyView ${url.pathname}: HTTP ${response.status} ${detail}`);
		}

		return response;
	}
}
