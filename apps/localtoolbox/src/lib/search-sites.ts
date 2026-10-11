// Where to look for a track nobody will sell you: the search badges.
//
// A track that offers neither a SoundCloud download nor a purchase link is a
// dead end as far as this extension is concerned, and the one useful thing
// left to say about it is "try somewhere else". So its row carries a lettered
// badge per site — a monogram in the site's colour rather than its logo, so
// the panel ships no third-party artwork and fetches nothing — and each badge
// opens a search there.
//
// Which sites those are is data, not code: ./search-sites.json is a list of
// plain objects, and adding one is an entry in it and nothing else. No
// manifest change either, because a badge opens a tab and never fetches, so
// none of these hosts is in `host_permissions`.
//
// An entry:
//
//   id          what the toggle for it is remembered under. Stable — renaming
//               it forgets whether somebody had turned that badge off.
//   name        the site's own name, as the settings sheet and the panel's
//               hints say it ("Bandcamp").
//   badge       the two or three characters on the button itself ("bc"). The
//               button is 28px wide; anything longer will not fit.
//   url         the search address, with {variables} in it (below).
//   background  the badge's colour, and
//   color       the ink on it, both as #rgb / #rrggbb. Optional: a site that
//               names neither gets a grey badge rather than an invisible one.
//   on          false to ship the badge switched off, for a site most people
//               will not want. Optional, and true when it is missing.
//
// The variables a `url` may carry are VARIABLES below: {artist}, {title} and
// {query}, each URL-encoded where it lands. An entry naming one that does not
// exist is dropped rather than searching for the word "artis" — see `siteOf`
// for everything else a dropped entry can be. A dropped entry is silent: this
// list is read to draw a panel, and a broken line in it must not be the reason
// the panel does not draw.
//
// Which badges a row actually carries is the other half, and it is a setting:
// ./search-sites.svelte.ts holds the switch per site, and the settings sheet
// is where they are thrown.

import type { Track } from '$lib/types';
import catalogue from './search-sites.json';
import { fallback, z } from './zod.ts';

type SearchTrack = Pick<Track, 'artist' | 'title'>;

/** A usable entry of the file, as `siteOf` makes it. */
export interface SearchSite {
	id: string;
	name: string;
	badge: string;
	url: string;
	/** The variables it uses, each one of VARIABLE_NAMES. */
	fields: VariableName[];
	/** The badge's inline style: its two colours. */
	style: string;
	on: boolean;
}

/**
 * The variables a URL template may carry, and what each is filled with.
 *
 * A track is a title and an artist and that is all this extension knows about
 * it, so the three of these are the artist, the title, and both — which is
 * what a shop's own search box is usually given.
 *
 * Keyed by name; `siteOf` keeps only a site whose every field is one of them.
 */
const VARIABLES = {
	artist: (track) => track.artist || '',
	title: (track) => track.title || '',
	query: (track) => [track.artist, track.title].filter(Boolean).join(' ')
} satisfies Record<string, (track: SearchTrack) => string>;

type VariableName = keyof typeof VARIABLES;

/** Own keys only: `'constructor' in VARIABLES` is true too. */
const isVariable = (name: string): name is VariableName => Object.hasOwn(VARIABLES, name);

/** The names a template may use, for the README and the tests to check. */
export const VARIABLE_NAMES = Object.keys(VARIABLES);

// `{artist}` rather than `${artist}`: the second is what a JavaScript template
// literal uses, and a string in the source carrying one is a mistake often
// enough that the linter says so (no-template-curly-in-string).
const PLACEHOLDER = /\{(\w+)\}/g;

// A colour as CSS understands it *and* as nothing else can be smuggled
// through: these land in a style attribute, and a hand-edited file is not the
// place to accept arbitrary declarations.
const HEX = /^#[0-9a-f]{3,8}$/i;

/** A member that should be words: trimmed, and '' for anything else. */
const Words = z.pipe(
	fallback(z.string(), ''),
	z.transform((value) => value.trim())
);

/** A colour member: one HEX takes, or `otherwise`. */
const colour = (otherwise: string) =>
	z.pipe(
		Words,
		z.transform((value) => (HEX.test(value) ? value : otherwise))
	);

/**
 * One entry of the file, every member checked as it is read: anything that is
 * not an object is null, and anything of the wrong type in one reads as missing.
 */
const Entry = fallback(
	z.nullable(
		z.object({
			id: Words,
			name: Words,
			badge: Words,
			url: Words,
			background: colour('#52525b'),
			color: colour('#fff'),
			on: z.optional(z.unknown())
		})
	),
	null
);

/**
 * One entry of the file as a site, or null for one that cannot be used: a
 * missing name or badge, an address that is not https, one with no variable
 * in it at all (a badge every row of which opens the same page), or one
 * naming a variable that does not exist.
 */
function siteOf(entry: z.output<typeof Entry>): SearchSite | null {
	if (!entry?.id || !entry.name || !entry.badge) return null;
	const { id, name, badge, url } = entry;

	if (!url.startsWith('https://')) return null;

	const fields = [...url.matchAll(PLACEHOLDER)].map((match) => match[1]);

	if (fields.length === 0) return null;

	if (!fields.every(isVariable)) return null;

	return {
		id,
		name,
		badge,
		url,
		/** The variables it uses, which is what decides whether a row can use it. */
		fields,
		style: `background:${entry.background};color:${entry.color}`,
		on: entry.on !== false
	};
}

/**
 * A list of entries as the sites it names, in the order it names them.
 *
 * `[entries].flat()` rather than a check that it is an array: a file somebody
 * has turned into one object is then one entry, which is refused below like
 * any other entry that does not describe a site.
 *
 * Two entries with one id would be two badges sharing a switch, so the second
 * is dropped, the likeliest way to write one being to copy the line above and
 * change half of it.
 */
const Sites = z.pipe(
	z.pipe(
		z.unknown(),
		z.transform((entries) => [entries].flat())
	),
	z.pipe(
		z.array(Entry),
		z.transform((entries) => {
			const sites = [];
			const seen = new Set();

			for (const entry of entries) {
				const site = siteOf(entry);

				if (!site) continue;

				if (seen.has(site.id)) continue;
				seen.add(site.id);
				sites.push(site);
			}

			return sites;
		})
	)
);

/** The file as parsed, or anything else, as the sites it names. */
export const readSites = Sites.parse.bind(Sites);

/** Every site the file describes, whether or not its badge is switched on. */
export const SEARCH_SITES = readSites(catalogue);

/**
 * What a badge searches for: the track's own words, in the order the template
 * asks for them. It is what the button's tooltip quotes, so that a badge says
 * what it is about to do rather than only where.
 */
export const queryOf = (site: SearchSite, track: SearchTrack) =>
	site.fields
		.map((field) => VARIABLES[field](track))
		.filter(Boolean)
		.join(' ');

/**
 * Whether a track says enough for this site's search to mean anything. A
 * playlist row with no artist on it leaves an {artist} badge with nothing to
 * search for, and the badge is drawn disabled rather than opening a search for
 * the empty string.
 */
export const searchable = (site: SearchSite, track: SearchTrack) =>
	site.fields.every((field) => VARIABLES[field](track) !== '');

/** The address a badge opens: the template, filled in and URL-encoded. */
export const searchUrl = (site: SearchSite, track: SearchTrack) =>
	// siteOf has kept only a URL whose every placeholder names a variable.
	site.url.replace(PLACEHOLDER, (_match, field: VariableName) => encodeURIComponent(VARIABLES[field](track)));

/** The sites named in a sentence: "Beatport, Bandcamp, ZIPDJ". */
export const siteNames = (sites: Pick<SearchSite, 'name'>[]) => sites.map((site) => site.name).join(', ');
