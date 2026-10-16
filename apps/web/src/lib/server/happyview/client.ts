import { setTimeout as sleep } from 'node:timers/promises';
import type { JsonObject } from '@atproto/lex-json';
import type { LexiconDoc } from '@atproto/lexicon';
import { z } from 'zod';
import type { MatchBreakdown, MatchNote } from '#lib/matching.ts';
import { DIRECTORY } from '#lib/nsid.ts';
import type { DjSetView } from '#lib/types.ts';
import { lexicons } from '../atproto/lexicons';
import type { HappyViewScript } from './bundle';

/** A published project as the directory serves it (`computer.localnetwork.directory.defs#projectView`). */
export type DirectoryProject = { uri: string; cid: string; record: JsonObject; distanceM?: number };

type Facet = { value: string; count: number };

type Place = { city: string; country: string; count: number };

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
	matchProjects: { matches: DirectoryMatch[] };
	getProject: { project?: DirectoryProject };
	getProjects: { projects: DirectoryProject[] };
	getStats: { total: number; active: number; unclaimed: number; cityCount: number; genres: Facet[]; cities: Place[] };
	getSets: { sets: DjSetView[] };
	listSets: { sets: DjSetView[] };
};

export type DirectoryStats = DirectoryOutputs['getStats'];

/** Query params before encoding. Arrays become repeated keys, the way XRPC sends them. */
type QueryParams = Record<string, string | number | boolean | string[] | undefined>;

const storedLexicon = z.object({ lexicon_json: z.json() });

const storedScripts = z.array(z.object({ id: z.string(), body: z.string() }));

const backfillJob = z.object({ id: z.string(), status: z.string() });

const BACKFILL_POLL_MS = 250;

const BACKFILL_DONE = new Set(['completed', 'failed', 'cancelled']);

/** The admin permissions `syncHappyView` and the set picker's backfills use, so HAPPYVIEW_API_KEY needs exactly these. */
export const HAPPYVIEW_PERMISSIONS = [
	'lexicons:read',
	'lexicons:create',
	'scripts:read',
	'scripts:manage',
	'backfill:create',
	'backfill:read'
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
 * Talks to the HappyView instance that indexes published projects and their managers' sets: directory
 * queries over XRPC, plus the admin calls that keep its lexicons and scripts in step with this app.
 */
export class HappyViewClient {
	readonly url: string;
	private readonly apiKey?: string;
	private readonly fetch: typeof globalThis.fetch;

	constructor(options: { url: string; apiKey?: string; fetch?: typeof globalThis.fetch }) {
		this.url = options.url;
		this.apiKey = options.apiKey;
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

	/** Has HappyView re-read one repo's records of `collection` from its PDS, through the record script. Returns the job id. */
	async backfill(collection: string, did: string): Promise<string> {
		const response = await this.request('/admin/backfill', { admin: true, body: { collection, did } });

		return backfillJob.parse(await response.json()).id;
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

	private async request(path: string, options: { admin?: boolean; body?: object } = {}) {
		const url = new URL(`${this.url}${path}`);
		const headers = new Headers();

		if (options.admin) {
			if (!this.apiKey) throw new Error('HappyView admin calls need HAPPYVIEW_API_KEY');
			headers.set('authorization', `Bearer ${this.apiKey}`);
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
