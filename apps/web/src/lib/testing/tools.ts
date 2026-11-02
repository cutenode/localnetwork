import type { Capture, Cue } from '#lib/tools/types.ts';

// A 1x1 PNG, so a test's captures load (and fire `load`) without a server.
const PIXEL =
	'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=';

const picture = (w: number, h: number) => ({
	sources: { avif: `${PIXEL} ${w}w`, webp: `${PIXEL} ${w}w` },
	img: { src: PIXEL, w, h }
});

/** A capture as `?enhanced` imports one: a `w` by `h` picture, here a pixel in every format, and maybe a phone's. */
export const capture = (alt: string, { w = 2560, h = 1536, phone = false } = {}): Capture => ({
	picture: picture(w, h),
	...(phone && { phone: picture(780, 1400) }),
	alt
});

export const cues: Cue[] = [
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
