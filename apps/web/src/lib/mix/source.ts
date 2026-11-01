import { COLLECTION } from '#lib/nsid.ts';
import { embedUrl, mixPlatform } from './links.ts';

/** plyr.fm's track record, which a set's `recording` can point at. */
export const PLYR_TRACK = 'fm.plyr.track';

/**
 * Where a set's recording plays, decided on the server (src/lib/server/services/recordings.ts). `audio` plays in the
 * page's own player, so the tracklist can follow it; `embed` is a platform's player in an iframe. `link` is the
 * recording's page elsewhere, for when it won't play here.
 */
export type PlayerSource =
	| { kind: 'audio'; src: string; from: 'plyr.fm' | 'pds' | 'archive.org'; artwork?: string; link?: string }
	| { kind: 'embed'; src: string; platform: string; link: string };

/** Files archive.org serves that browsers play. */
const ARCHIVE_AUDIO = /\.(mp3|m4a|ogg|oga|opus|flac|wav)$/i;

/** An archive.org download (`https://archive.org/download/<item>/<file>`) of an audio file, played as it is. */
function archiveAudio(uri: string): PlayerSource | null {
	let url: URL;

	try {
		url = new URL(uri);
	} catch {
		return null;
	}

	const [, root, item, ...file] = url.pathname.split('/');

	const isDownload =
		url.protocol === 'https:' && url.hostname === 'archive.org' && root === 'download' && item && file.length;

	if (!isDownload || !ARCHIVE_AUDIO.test(url.pathname)) return null;

	return { kind: 'audio', src: url.href, from: 'archive.org', link: `https://archive.org/details/${item}` };
}

/** What each of a set's links can play: an archive.org file directly, or a platform's player. */
export function linkSources(links: { uri: string }[] | undefined): PlayerSource[] {
	return (links ?? []).flatMap(({ uri }) => {
		const archive = archiveAudio(uri);

		if (archive) return [archive];
		const embed = embedUrl(uri);

		return embed ? [{ kind: 'embed' as const, src: embed, platform: mixPlatform(uri) ?? 'the web', link: uri }] : [];
	});
}

/**
 * The source to play, in order of preference: audio over a platform's player, since only audio lets the tracklist
 * follow along everywhere, and otherwise the order given.
 */
export function preferred(sources: (PlayerSource | null)[]): PlayerSource | null {
	const found = sources.filter((source): source is PlayerSource => source !== null);

	return found.find((source) => source.kind === 'audio') ?? found[0] ?? null;
}

/** Whether a set may have something to play, from the record alone, before the server resolves its recording. */
export function playable(set: { recording?: { uri: string }; links?: { uri: string }[] }): boolean {
	const collection = set.recording?.uri.split('/')[3];

	return collection === PLYR_TRACK || collection === COLLECTION.djRecording || linkSources(set.links).length > 0;
}

/** A track the player follows: its start in the recording, when the set knows it. */
export type PlayerTrack = { title: string; artist?: string; startSeconds?: number };

/** The track playing `seconds` into the recording: the last one started by then, or -1 before the first. */
export function trackAt(tracks: PlayerTrack[], seconds: number): number {
	let current = -1;

	for (const [i, track] of tracks.entries())
		if (
			track.startSeconds !== undefined &&
			track.startSeconds <= seconds &&
			(current < 0 || track.startSeconds >= (tracks[current].startSeconds ?? 0))
		)
			current = i;

	return current;
}
