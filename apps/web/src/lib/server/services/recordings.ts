import { z } from 'zod';
import type { DjSet } from '#lib/dj.ts';
import { djSetSchema } from '#lib/dj-schema.ts';
import { linkSources, PLYR_TRACK, type PlayerSource, preferred } from '#lib/mix/source.ts';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProjectMix } from '#lib/types.ts';
import { parsePublicRecordUri } from '../atproto/spaces';
import { RECORDING_MAX_BYTES, RECORDING_TYPES } from '../audio';
import type { IndexedRepo } from '../happyview/indexed';
import type { Logger } from '../logger';
import type { PlyrClient } from '../plyr';

/**
 * How long a lookup the audio route makes is reused. A playing <audio> asks for a new range on every seek, and each
 * would otherwise list the account's recordings again.
 */
const FRESH_MS = 60_000;

const CACHE_SIZE = 500;

/** Pages of an account's recordings the audio route reads, 100 to a page. */
const RECORDING_PAGES = 5;

/**
 * The part of a `computer.localnetwork.dj.recording` the site reads: its audio, of a type and size the lexicon takes.
 * The player reads how long it plays from the audio itself, so a record without `durationSeconds` plays the same.
 */
const recordingSchema = z.object({
	audio: z.object({
		$type: z.literal('blob'),
		ref: z.object({ $link: z.string() }),
		// The audio route sends it as the Content-Type, so it's a bare type and nothing more.
		mimeType: z
			.string()
			.regex(/^[\w.+-]+\/[\w.+-]+$/)
			.refine((mime) => RECORDING_TYPES.matches({ mime })),
		size: z.number().int().min(1).max(RECORDING_MAX_BYTES)
	})
});

/** A recording's audio blob as its record declares it, which the audio route serves by. */
export type RecordingAudio = { mimeType: string; size: number };

/** A small in-memory cache with a lifetime, dropping the oldest entry past its size. */
class Recent<T> {
	private readonly entries = new Map<string, { at: number; value: T }>();

	constructor(private readonly now: () => number) {}

	async get(key: string, load: () => Promise<T>): Promise<T> {
		const cached = this.entries.get(key);

		if (cached && this.now() - cached.at < FRESH_MS) return cached.value;
		const value = await load();
		this.entries.set(key, { at: this.now(), value });

		if (this.entries.size > CACHE_SIZE) this.entries.delete(this.entries.keys().next().value!);

		return value;
	}
}

/**
 * Where a set's recording plays, decided when someone presses Play: its `recording` record (an `fm.plyr.track`, asked
 * of plyr.fm, or a `computer.localnetwork.dj.recording` on the DJ's PDS, served by /media/audio), then its links.
 * Records are read through HappyView. Recordings on a PDS play only for accounts that manage an active project, since
 * the site streams their bytes.
 */
export class RecordingService {
	private readonly managers: Recent<boolean>;

	private readonly audios: Recent<RecordingAudio | null>;

	constructor(
		private readonly repos: IndexedRepo,
		private readonly plyr: PlyrClient,
		/** Whether the account manages an active project. */
		private readonly manages: (did: string) => Promise<boolean>,
		private readonly logger: Logger,
		now: () => number = Date.now
	) {
		this.managers = new Recent(now);
		this.audios = new Recent(now);
	}

	/** A DJ's set as HappyView has it, or null when it doesn't (or it doesn't parse). */
	async set(did: string, rkey: string): Promise<DjSet | null> {
		// A route's DID and record key go to HappyView only once they make an at:// URI.
		if (!parsePublicRecordUri(`at://${did}/${COLLECTION.djSet}/${rkey}`)) return null;
		const record = await this.repos.getRecord(did, COLLECTION.djSet, rkey);

		if (!record) return null;
		const parsed = djSetSchema.safeParse(record.value);

		return parsed.success ? { ...parsed.data, uri: record.uri } : null;
	}

	/** What a set plays: its recording record, then its links, audio first. */
	async source(set: Pick<DjSet, 'recording' | 'links'>): Promise<PlayerSource | null> {
		const recording = set.recording ? await this.recording(set.recording.uri) : null;

		return preferred([recording, ...linkSources(set.links)]);
	}

	/**
	 * What a mix on a profile plays: its linked set's recording or links, else the mix's own link. Never rejects, since
	 * the mix's page streams it.
	 */
	async forMix(mix: Pick<ProjectMix, 'url' | 'set'>): Promise<PlayerSource | null> {
		const own = linkSources([{ uri: mix.url }]);

		try {
			const ref = mix.set && parsePublicRecordUri(mix.set.uri);
			const set = ref && ref.collection === COLLECTION.djSet ? await this.set(ref.did, ref.rkey) : null;

			return preferred([set && (await this.source(set)), ...own]);
		} catch (error) {
			this.logger.warn({ err: error, set: mix.set?.uri }, "could not find a mix's linked recording");

			return preferred(own);
		}
	}

	/** The audio of an indexed recording of `did` whose blob is `cid`, or null when there's none the site serves. */
	audio(did: string, cid: string): Promise<RecordingAudio | null> {
		return this.audios.get(`${did} ${cid}`, async () => {
			if (!(await this.isManager(did))) return null;
			const records = await this.repos.listAll(did, COLLECTION.djRecording, { pages: RECORDING_PAGES });

			for (const record of records) {
				const parsed = recordingSchema.safeParse(record.value);

				if (parsed.success && parsed.data.audio.ref.$link === cid)
					return { mimeType: parsed.data.audio.mimeType, size: parsed.data.audio.size };
			}

			return null;
		});
	}

	private async recording(uri: string): Promise<PlayerSource | null> {
		const ref = parsePublicRecordUri(uri);

		if (ref?.collection === PLYR_TRACK) return this.plyrTrack(uri);

		if (ref?.collection === COLLECTION.djRecording) return this.pdsRecording(ref.did, uri);

		return null;
	}

	/** A plyr.fm track plays from plyr's CDN, or in plyr's own player when its file isn't public. */
	private async plyrTrack(uri: string): Promise<PlayerSource | null> {
		const track = await this.plyr.track(uri);

		if (!track) return null;
		const link = `https://plyr.fm/track/${track.id}`;

		if (!track.audio)
			return { kind: 'embed', src: `https://plyr.fm/embed/track/${track.id}`, platform: 'plyr.fm', link };

		return { kind: 'audio', src: track.audio, from: 'plyr.fm', artwork: track.artwork, link };
	}

	private async pdsRecording(did: string, uri: string): Promise<PlayerSource | null> {
		const record = (await this.repos.getRecords([uri])).get(uri);
		const parsed = record && recordingSchema.safeParse(record.value);

		if (!parsed?.success) return null;

		if (!(await this.isManager(did))) return null;

		return { kind: 'audio', src: `/media/audio/${did}/${parsed.data.audio.ref.$link}`, from: 'pds' };
	}

	/** Whether `did` manages an active project. A directory that can't say reads as no, and isn't remembered. */
	private async isManager(did: string): Promise<boolean> {
		try {
			return await this.managers.get(did, () => this.manages(did));
		} catch (error) {
			this.logger.warn({ err: error, did }, 'could not check whether an account manages a project');

			return false;
		}
	}
}
