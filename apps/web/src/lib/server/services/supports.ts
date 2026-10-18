import type { ChartFilters } from '#lib/supports.ts';
import type { ArtistSupports, Charts, SupportedArtist } from '#lib/types.ts';
import { type DirectoryOutputs, type HappyViewClient, QUERY_BYTES } from '../happyview/client';
import type { Logger } from '../logger';
import { truncateUtf8 } from '../utf8';
import type { AccountService } from './accounts';

/** Tracks and artists the charts show. */
const CHART_SIZE = 50;

/** Artists a search shows. */
const SEARCH_SIZE = 30;

/** The longest artist and city getArtist and getCharts take, in UTF-8 bytes (their lexicons' maxLength). */
const ARTIST_BYTES = 2560;

const CITY_BYTES = 200;

/** How long an answer is reused, so a busy page doesn't have the directory count every set again on each view. */
const FRESH_MS = 60_000;

const MEMO_SIZE = 200;

type Query = 'getCharts' | 'getArtist' | 'searchArtists';

/**
 * Who played whose tracks, from the sets the directory keeps (lua/computer/localnetwork/directory/getCharts.lua,
 * getArtist.lua and searchArtists.lua count them). Reads only; a HappyViewError means the directory is down, which
 * pages show as such.
 */
export class SupportService {
	private readonly memo = new Map<string, { at: number; answer: Promise<DirectoryOutputs[Query]> }>();

	constructor(
		private readonly directory: HappyViewClient,
		private readonly accounts: AccountService,
		private readonly logger: Logger,
		private readonly now: () => number = Date.now
	) {}

	charts(filters: ChartFilters): Promise<Charts> {
		const { city } = filters.query;

		return this.query('getCharts', {
			...filters.query,
			city: city && truncateUtf8(city, CITY_BYTES),
			limit: CHART_SIZE
		});
	}

	/** Artists whose name has `q` in it, or the most supported ones without it. */
	async search(q: string): Promise<SupportedArtist[]> {
		const { artists } = await this.query('searchArtists', {
			q: truncateUtf8(q, QUERY_BYTES) || undefined,
			limit: SEARCH_SIZE
		});

		return artists;
	}

	/** An artist's tracks and every play of them, by MBID or name, with each DJ's handle. */
	async artist(artist: { mbid: string } | { name: string }): Promise<ArtistSupports> {
		const params = 'mbid' in artist ? artist : { name: truncateUtf8(artist.name, ARTIST_BYTES) };
		const { djs, ...supports } = await this.query('getArtist', params);
		const handles = await this.accounts.views(djs.map((dj) => dj.did));
		this.logger.debug({ artist, tracks: supports.tracks.length, djs: djs.length }, 'artist supports');

		return {
			...supports,
			djs: Object.fromEntries(djs.map((dj) => [dj.did, { ...dj, handle: handles.get(dj.did)?.handle ?? null }]))
		};
	}

	/** A directory query, or the same one's answer from the last minute. A failed one isn't kept. */
	private query<K extends Query>(
		name: K,
		params: Record<string, string | number | undefined>
	): Promise<DirectoryOutputs[K]> {
		const key = JSON.stringify([name, params]);
		const kept = this.memo.get(key);

		if (kept && this.now() - kept.at < FRESH_MS) {
			// SAFETY: the key starts with the query's name, so a kept answer is that query's.
			return kept.answer as Promise<DirectoryOutputs[K]>;
		}

		const answer = this.directory.query(name, params);
		this.memo.delete(key);
		this.memo.set(key, { at: this.now(), answer });

		if (this.memo.size > MEMO_SIZE) this.memo.delete(this.memo.keys().next().value!);
		answer.catch(() => {
			if (this.memo.get(key)?.answer === answer) this.memo.delete(key);
		});

		return answer;
	}
}
