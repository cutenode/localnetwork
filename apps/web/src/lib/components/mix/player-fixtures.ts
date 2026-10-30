import type { PlayerSource, PlayerTrack } from '#lib/mix/source.ts';

/** Made-up recordings for the player's stories and tests, which play without fetching anything. */

const RATE = 8_000;

/** A silent WAV `seconds` long (8 kHz, 8-bit mono), as an object URL. */
export function silence(seconds: number): string {
	const samples = RATE * seconds;
	const bytes = new Uint8Array(44 + samples);
	const view = new DataView(bytes.buffer);
	const ascii = (at: number, text: string) => bytes.set(new TextEncoder().encode(text), at);

	ascii(0, 'RIFF');
	view.setUint32(4, 36 + samples, true);
	ascii(8, 'WAVE');
	ascii(12, 'fmt ');
	view.setUint32(16, 16, true);
	view.setUint16(20, 1, true);
	view.setUint16(22, 1, true);
	view.setUint32(24, RATE, true);
	view.setUint32(28, RATE, true);
	view.setUint16(32, 1, true);
	view.setUint16(34, 8, true);
	ascii(36, 'data');
	view.setUint32(40, samples, true);
	// Unsigned 8-bit samples are silent at the midpoint.
	bytes.fill(128, 44);

	return URL.createObjectURL(new Blob([bytes], { type: 'audio/wav' }));
}

export const tracks: PlayerTrack[] = [
	{ title: 'Night Bus', artist: 'Ferrule', startSeconds: 0 },
	{ title: 'Low Tide (Dub)', artist: 'Ferrule, Ava Lind', startSeconds: 95 },
	{ title: 'Unreleased', startSeconds: 180 },
	{ title: 'Halogen', artist: 'Two Rooms', startSeconds: 250 },
	{ title: 'Backroom Glow', artist: 'Mara Velez' }
];

/** The source the server would pick for each kind of recording, playing `src`. */
export const sources = (src: string): Record<'pds' | 'plyr' | 'archive' | 'embed', PlayerSource> => ({
	pds: { kind: 'audio', src, from: 'pds' },
	plyr: { kind: 'audio', src, from: 'plyr.fm', link: 'https://plyr.fm/track/1567' },
	archive: { kind: 'audio', src, from: 'archive.org', link: 'https://archive.org/details/tidal-hours' },
	embed: { kind: 'embed', src: 'about:blank', platform: 'YouTube', link: 'https://www.youtube.com/watch?v=tidal' }
});
