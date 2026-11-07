import type { Capture, Cue } from '#lib/tools/types.ts';

// A 1x1 PNG, so a test's captures load (and fire `load`) without a server.
const PIXEL =
	'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=';

const picture = (w: number, h: number, src = PIXEL) => ({
	sources: { avif: `${src} ${w}w`, webp: `${src} ${w}w`, jpeg: `${src} ${w}w` },
	img: { src, w, h }
});

/**
 * A capture as `?enhanced` imports one: a `w` by `h` picture, here a pixel in every format, and maybe
 * a phone's. `src` is another image to use, for a test that holds back or breaks one capture's load.
 */
export const capture = (alt: string, { w = 2560, h = 1536, phone = false, src = PIXEL } = {}): Capture => ({
	picture: picture(w, h, src),
	...(phone && { phone: picture(780, 1400, src) }),
	alt
});

export const cues: [Cue, ...Cue[]] = [
	{ name: 'Sync', line: 'Keeps drives in sync', capture: capture('The drives view', { phone: true }) },
	{ name: 'Edit', line: 'Edits playlists on the drive', capture: capture('The library view') },
	{ name: 'Check', line: 'Checks drives before a set', capture: capture('The check report', { w: 800, h: 1160 }) }
];

/** A track with three phrases and an overview of eight columns. */
export const track = {
	durationSecs: 240,
	overview: { heights: [40, 80, 120, 160, 200, 160, 120, 80], colors: Array<string>(8).fill('#2050ff') },
	phrases: [
		{ start: 0, end: 80, kind: 'Intro' },
		{ start: 80, end: 160, kind: 'Chorus' },
		{ start: 160, end: 240, kind: 'Outro' }
	]
};
