import { type DjSet, setPlace } from '#lib/dj.ts';
import { musicBrainzUrl, trackArtists } from '#lib/supports.ts';

/** The Podcasting 2.0 JSON chapters format's media type. */
export const CHAPTERS_TYPE = 'application/json+chapters';

type Chapter = { startTime: number; title: string; url?: string };

/** A chapters document (version 1.2.0): the set's name, when it has one, and a chapter per timed track. */
export type Chapters = { version: '1.2.0'; title?: string; chapters: Chapter[] };

/**
 * A set's tracklist as Podcasting 2.0 JSON chapters (version 1.2,
 * https://github.com/Podcastindex-org/podcast-namespace/blob/main/docs/examples/chapters/jsonChapters.md), so a podcast
 * app playing the recording shows each track as it comes up. Only tracks with a start become chapters, in order.
 */
export function setChapters(set: DjSet): Chapters {
	const chapters = set.tracks
		.flatMap((track): Chapter[] => {
			if (track.startSeconds === undefined) return [];

			if (track.unidentified) return [{ startTime: track.startSeconds, title: 'Unreleased' }];

			const artists = trackArtists(track)
				.map((artist) => artist.name)
				.join(', ');

			const title = [artists, track.title ?? 'Untitled'].filter(Boolean).join(' - ');
			const url = track.recordingMbId && musicBrainzUrl('recording', track.recordingMbId);

			return [{ startTime: track.startSeconds, title, ...(url && { url }) }];
		})
		.sort((a, b) => a.startTime - b.startTime);

	const title = set.title ?? setPlace(set) ?? undefined;

	return { version: '1.2.0', ...(title && { title }), chapters };
}
