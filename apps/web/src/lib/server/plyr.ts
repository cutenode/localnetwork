import { z } from 'zod';
import type { StrongRef } from '#lib/types.ts';
import type { Logger } from './logger';

const API = 'https://api.plyr.fm';

/** Where plyr.fm serves public audio (its CDN), the one audio host the site's CSP allows for plyr. */
const AUDIO_ORIGIN = 'https://audio.plyr.fm';

const TIMEOUT_MS = 5_000;

/** How long an answer is reused, so a set played again soon doesn't ask plyr.fm again. */
const FRESH_MS = 5 * 60_000;

const CACHE_SIZE = 200;

/** The parts of plyr.fm's TrackResponse the player uses. */
const trackSchema = z.object({
	id: z.number().int(),
	title: z.string(),
	artist: z.string(),
	image_url: z.string().nullish(),
	file_id: z.string(),
	support_gate: z.unknown().optional(),
	gated: z.boolean().optional(),
	visibility: z.string().optional()
});

/** Where plyr.fm's TrackResponse names the track's own record, which it has once it has written it. */
const recordSchema = z.object({ atproto_record_uri: z.string().nullish(), atproto_record_cid: z.string().nullish() });

/** A plyr.fm track: `audio` is its public file on plyr's CDN, missing when it's gated or private. */
export type PlyrTrack = { id: number; title: string; artist: string; artwork?: string; audio?: string };

/**
 * plyr.fm's public API (https://api.plyr.fm/docs), asked on demand when someone plays a set whose recording is an
 * `fm.plyr.track`. Answers are cached in memory for a few minutes. A failure reads as no track, so the player falls
 * back to the set's other links.
 */
export class PlyrClient {
	private readonly cache = new Map<string, { at: number; value: PlyrTrack | null }>();

	constructor(
		private readonly logger: Logger,
		private readonly fetch: typeof globalThis.fetch = globalThis.fetch,
		private readonly now: () => number = Date.now
	) {}

	/** The track whose record is `uri` (an `at://` URI), or null when plyr.fm doesn't have it or can't answer. */
	async track(uri: string): Promise<PlyrTrack | null> {
		const cached = this.cache.get(uri);

		if (cached && this.now() - cached.at < FRESH_MS) return cached.value;

		let value: PlyrTrack | null;

		try {
			value = await this.lookup(uri);
		} catch (error) {
			// Not cached, so the next play asks again.
			this.logger.warn({ err: error, uri }, 'could not look up a plyr.fm track');

			return null;
		}

		this.cache.set(uri, { at: this.now(), value });

		if (this.cache.size > CACHE_SIZE) this.cache.delete(this.cache.keys().next().value!);

		return value;
	}

	/**
	 * The record of the track whose ID is `id` (from its link, `https://plyr.fm/track/<id>`), or null when plyr.fm
	 * doesn't have it, hasn't written it yet, or can't answer. Asked when a mix links the track, so it isn't cached.
	 */
	async record(id: string): Promise<StrongRef | null> {
		try {
			const track = await this.get(`/tracks/${encodeURIComponent(id)}`, recordSchema);
			const uri = track?.atproto_record_uri;
			const cid = track?.atproto_record_cid;

			return uri && cid ? { uri, cid } : null;
		} catch (error) {
			this.logger.warn({ err: error, id }, "could not look up a plyr.fm track's record");

			return null;
		}
	}

	private async lookup(uri: string): Promise<PlyrTrack | null> {
		const track = await this.get(`/tracks/by-uri?uri=${encodeURIComponent(uri)}`, trackSchema);

		if (!track) return null;
		const restricted = track.gated || track.support_gate != null || track.visibility === 'private';
		const audio = restricted ? undefined : await this.audio(track.file_id);

		return { id: track.id, title: track.title, artist: track.artist, artwork: track.image_url ?? undefined, audio };
	}

	/** The track's file on plyr's CDN. An unmirrored track's answer is its PDS, which the CSP doesn't allow. */
	private async audio(fileId: string): Promise<string | undefined> {
		try {
			const found = await this.get(`/audio/${encodeURIComponent(fileId)}/url`, z.object({ url: z.url() }));

			return found && new URL(found.url).origin === AUDIO_ORIGIN ? found.url : undefined;
		} catch (error) {
			// The track still plays in plyr's own player.
			this.logger.warn({ err: error, fileId }, 'could not find the audio of a plyr.fm track');

			return undefined;
		}
	}

	/** A JSON answer as `schema` reads it, or null for a 404 (and a 401 or 402, which a gated track's file answers). */
	private async get<T>(path: string, schema: z.ZodType<T>): Promise<T | null> {
		const response = await this.fetch(`${API}${path}`, {
			headers: { accept: 'application/json' },
			signal: AbortSignal.timeout(TIMEOUT_MS)
		});

		if ([401, 402, 404].includes(response.status)) return null;

		if (!response.ok) throw new Error(`plyr.fm ${path.split('?')[0]}: HTTP ${response.status}`);

		return schema.parse(await response.json());
	}
}
