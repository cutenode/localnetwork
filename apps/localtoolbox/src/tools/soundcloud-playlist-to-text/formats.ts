// Turning a page's tracks into text.
//
// All of it happens here in the extension's own popup — never in the page.
// The content script only reads raw strings out of the DOM; the splitting,
// the deduping and every output format below work on what it read.

import type { Track } from '$lib/types';

/**
 * A track as the panel hands it over: its artist and title, and nothing else. A
 * page's tracks always carry both; a queue entry need not (see QueueEntry).
 */
export type Listed = Pick<Track, 'artist' | 'title'>;

/** A track as a format reads it: '' for an artist or title the track never named. */
export type Line = Required<Listed>;

export interface Format {
	/** The pref's value for it. */
	value: string;
	label: string;
	/** The saved file's extension. */
	ext: string;
	mime: string;
	render: (tracks: Line[]) => string;
}

// Dash separators commonly used inside track titles ("Artist - Title").
const TITLE_SEPARATORS = [' - ', ' – ', ' — '];

/**
 * Many SoundCloud uploads are posted by channels, with the real artist inside
 * the track title. Splits at the earliest separator of any kind, so a title
 * mixing dash variants ("Artist – Song - Official Video") splits at the
 * artist boundary rather than the later one.
 *
 * @returns the track itself where there is nothing to split
 */
export function splitTitleArtist(track: Line): Line {
	let at = -1;
	let sepLength = 0;

	for (const sep of TITLE_SEPARATORS) {
		const i = track.title.indexOf(sep);

		if (i > 0 && i + sep.length < track.title.length && (at === -1 || i < at)) {
			at = i;
			sepLength = sep.length;
		}
	}

	if (at === -1) return track;

	return {
		artist: track.title.slice(0, at).trim(),
		title: track.title.slice(at + sepLength).trim()
	};
}

export function dedupeTracks(tracks: Line[]) {
	const seen = new Set();

	return tracks.filter((t) => {
		// NUL delimiter: a plain-space join could collide when the artist/title
		// boundary shifts ("Bicep"+"Glue Remix" vs "Bicep Glue"+"Remix").
		const key = `${t.artist}\u0000${t.title}`.toLowerCase();

		if (seen.has(key)) return false;
		seen.add(key);

		return true;
	});
}

// A bare \r ends a row as surely as \n does, to a spreadsheet and to most
// CSV readers, so it is quoted too.
const csvField = (s: string) => (/[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s);

// The line formats promise a track per line, and TSV has no quoting at all: a
// tab or a line break inside a field is read as the next field or the next
// row. A page read never carries one — the content script collapses
// whitespace — but a queue restored from a state file can, so they become a
// space here rather than a broken line.
const oneLine = (s: string) => s.replace(/[\r\n]+/g, ' ');

const tsvField = (s: string) => s.replace(/[\t\r\n]+/g, ' ');

/** The output formats, in the order the menu offers them. */
export const FORMATS: Format[] = [
	{
		value: 'artist-title',
		label: 'artist - title',
		ext: 'txt',
		mime: 'text/plain',
		render: (ts) => ts.map((t) => oneLine(`${t.artist} - ${t.title}`)).join('\n')
	},
	{
		value: 'title-artist',
		label: 'title - artist',
		ext: 'txt',
		mime: 'text/plain',
		render: (ts) => ts.map((t) => oneLine(`${t.title} - ${t.artist}`)).join('\n')
	},
	{
		value: 'title-only',
		label: 'title only',
		ext: 'txt',
		mime: 'text/plain',
		render: (ts) => ts.map((t) => oneLine(t.title)).join('\n')
	},
	{
		value: 'numbered',
		label: 'numbered list',
		ext: 'txt',
		mime: 'text/plain',
		render: (ts) => ts.map((t, i) => oneLine(`${i + 1}. ${t.artist} - ${t.title}`)).join('\n')
	},
	{
		value: 'csv',
		label: 'CSV',
		ext: 'csv',
		mime: 'text/csv',
		render: (ts) => ['Artist,Title', ...ts.map((t) => `${csvField(t.artist)},${csvField(t.title)}`)].join('\n')
	},
	{
		value: 'tsv',
		label: 'TSV',
		ext: 'tsv',
		mime: 'text/tab-separated-values',
		render: (ts) => ['Artist\tTitle', ...ts.map((t) => `${tsvField(t.artist)}\t${tsvField(t.title)}`)].join('\n')
	},
	{
		value: 'json',
		label: 'JSON',
		ext: 'json',
		mime: 'application/json',
		// Track objects from the content script carry extra fields (artworkUrl,
		// used by the Cover Art tool); this tool's JSON output stays artist+title.
		render: (ts) =>
			JSON.stringify(
				ts.map(({ artist, title }) => ({ artist, title })),
				null,
				2
			)
	}
];

export const FORMAT_VALUES = FORMATS.map((format) => format.value);

export const formatFor = (value: string) => FORMATS.find((format) => format.value === value) ?? FORMATS[0];

// A queue entry that names no artist or title reads as '' does, which is what
// a row that named none gives: not "undefined" in the text, nor a throw.
const lineOf = ({ artist = '', title = '' }: Listed): Line => ({ artist, title });

/**
 * The tracks as the options say they should be, ready to render.
 * @param tracks the page's, or the queue's
 */
export function processTracks(tracks: Listed[], { splitTitle = false, dedupe = false } = {}) {
	let out = tracks.map(lineOf);

	if (splitTitle) out = out.map(splitTitleArtist);

	if (dedupe) out = dedupeTracks(out);

	return out;
}

/** @param s the page's title */
export function slugify(s?: string | null) {
	const slug = (s || '')
		.toLowerCase()
		.normalize('NFKD')
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '');

	// Generic fallback: the title may be empty on either a playlist or a Likes page.
	return slug || 'soundcloud-tracks';
}
