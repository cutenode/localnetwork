// Turning the Manager's listed tracks and their download details into a list
// of links.
//
// A track can offer none, one or both of: a SoundCloud download (the
// "Download file" item in its More menu) and a purchase link (its "Buy" /
// "Free Download" button). Sorted **by site**, those links are grouped by
// where they point, soundcloud.com first; sorted in **list order**, every
// track appears once with all of its links. Tracks that turn out to offer
// nothing at all are listed on their own, with searches to try instead, and
// tracks nobody has asked SoundCloud about yet say so rather than pretending
// to be either.

import { titleCase } from '$lib/title-case.ts';
import type { ListedEntry, DetailsAnswer, LookupError } from '$lib/types';

/** One link a track offers, as a row draws it. */
export interface LinkEntry {
	track: ListedEntry;
	/**
	 * 'soundcloud' opens the track page, whose More menu holds the download; 'external' is the
	 * purchase link.
	 */
	kind: 'soundcloud' | 'external';
	url: string;
	host: string;
	/** The site it is grouped under. */
	domain: string;
	label: string;
	/** A SoundCloud download whose limit is used up. */
	spent: boolean;
	/** What the row's detail line says beside the host, or ''. */
	note: string;
}

// SoundCloud wraps outbound links in its gate.sc redirector
// (https://gate.sc?url=<encoded>&token=…). The API returns the raw purchase
// URL, but unwrap defensively so grouping is always by the real destination.
export function unwrapGate(url: string) {
	try {
		const parsed = new URL(url);

		if (parsed.hostname === 'gate.sc') {
			const inner = parsed.searchParams.get('url');

			if (inner) return inner;
		}
	} catch (_e) {
		// Not a URL; returned as is below.
	}

	return url;
}

export function hostOf(url: string) {
	try {
		return new URL(url).hostname.replace(/^www\./, '').toLowerCase();
	} catch (_e) {
		return '';
	}
}

// Country-code second-level suffixes under which the site name is the third
// label (artist.bandcamp.co.uk → bandcamp.co.uk). Kept short on purpose.
const TWO_LEVEL_SUFFIXES = new Set([
	'co.uk',
	'org.uk',
	'ac.uk',
	'co.jp',
	'ne.jp',
	'com.au',
	'net.au',
	'org.au',
	'com.br',
	'com.mx',
	'com.ar',
	'co.nz',
	'co.za',
	'co.kr',
	'com.tr',
	'com.sg',
	'com.hk',
	'co.in',
	'co.il'
]);

// The site a host belongs to: its registrable domain (dani3l.bandcamp.com →
// bandcamp.com), so one artist's subdomain does not become its own group.
export function domainOf(host: string) {
	const parts = host.split('.');

	if (parts.length <= 2) return host;
	const lastTwo = parts.slice(-2).join('.');

	return TWO_LEVEL_SUFFIXES.has(lastTwo) ? parts.slice(-3).join('.') : lastTwo;
}

// Where to look for a track that has no download link at all: a search on each
// of the sites somebody has switched on, as a lettered badge in the site's
// colour. The sites are data and the switches are a setting, so neither is
// here — see $lib/search-sites.js and $lib/search-sites.svelte.js.

export const SOUNDCLOUD_HINT =
	'SoundCloud downloads live in each track’s More menu (More → Download file). using it marks ' +
	'the track downloaded.';

/**
 * A track's download links as list entries: none, one or both. Tracks without
 * details, or without a permalink, have none.
 */
export function entriesFor(track: ListedEntry) {
	const details = track.details;
	const entries: LinkEntry[] = [];

	if (!details || !track.permalink) return entries;

	if (details.downloadable === true) {
		// Spent: SoundCloud still offers the download, but its limit for the
		// period is used up, so the button is there and gives nothing.
		const spent = !details.hasDownloadsLeft;
		entries.push({
			track,
			kind: 'soundcloud',
			url: `https://soundcloud.com${track.permalink}`,
			host: 'soundcloud.com',
			domain: 'soundcloud.com',
			label: 'open track',
			spent,
			note: spent ? 'no downloads left' : ''
		});
	}

	if (details.purchaseUrl) {
		const url = unwrapGate(details.purchaseUrl);
		const host = hostOf(url);

		if (host) {
			entries.push({
				track,
				kind: 'external',
				url,
				host,
				domain: domainOf(host),
				// SoundCloud's own label for the link, from the API or from the
				// page's button ("Buy" when the uploader gave none).
				label: details.purchaseTitle || track.domPurchaseTitle || 'open link',
				spent: false,
				note: ''
			});
		}
	}

	return entries;
}

// Where an entry stands when a track offers both links. A row showing two
// buttons of the same weight has nothing left over for the title beside them,
// so one link leads and the other trails: the SoundCloud download, being the
// file itself with no gate to walk, unless its limit is used up — a button
// that gives nothing is not the one to put forward, and then the purchase
// link leads instead.
const rank = (entry: LinkEntry) => (entry.kind !== 'soundcloud' ? 1 : entry.spent ? 2 : 0);

/**
 * A row's entries as the row shows them: `ordered` best first, `primary` the
 * one to make a button of, `secondary` the rest — none of them, or the one
 * other link, shown smaller beside it. `primary` is null only when the track
 * has no links at all.
 */
export function splitEntries(entries: LinkEntry[]) {
	const ordered = [...entries].sort((a, b) => rank(a) - rank(b));
	const [primary = null, ...secondary] = ordered;

	return { ordered, primary, secondary };
}

/**
 * Entries grouped by site, soundcloud.com first, then the busiest sites, then
 * alphabetical. A track can appear twice (a SoundCloud download and a purchase
 * link), in two groups.
 *
 * @returns [domain, its entries] pairs.
 */
export function buildGroups(tracks: ListedEntry[]): [string, LinkEntry[]][] {
	const groups = new Map<string, LinkEntry[]>();

	for (const track of tracks) {
		for (const entry of entriesFor(track)) {
			if (!groups.has(entry.domain)) groups.set(entry.domain, []);
			groups.get(entry.domain)!.push(entry);
		}
	}

	return [...groups.entries()].sort(([a, aEntries], [b, bEntries]) => {
		if (a === 'soundcloud.com') return -1;

		if (b === 'soundcloud.com') return 1;

		return bEntries.length - aEntries.length || a.localeCompare(b);
	});
}

// A track whose details are fully known and offer no download link at all.
// (`downloadable` is null when only the page's purchase link is known; those
// are never called link-less.)
export function isWithoutLinks(track: ListedEntry) {
	return !!track.permalink && !!track.details && track.details.downloadable === false && !track.details.purchaseUrl;
}

export const tracksWithoutLinks = (tracks: ListedEntry[]) => tracks.filter(isWithoutLinks);

// A track the list has no answer about: no details at all, or an answer that
// does not settle the question — a Likes row read straight off the page that
// rendered no purchase link says nothing about SoundCloud's own download
// either, which is half the question and the half nobody asked.
//
// The three kinds of row are this, `isWithoutLinks`, and having entries, and
// they are exhaustive and disjoint by construction: a track with no entries
// that is not known to be link-less is one nobody has finished asking about.
//
// On the old panel this could only mean one thing — a lookup that was running,
// or one that had just failed — because the list *was* the page and every
// track on it had been asked about the moment it appeared. A list that is kept
// holds another kind: a track that has simply never been checked, because the
// page it came in from was added before it, or because it came from the queue
// on a tab that was not SoundCloud at all. Which of the two a given track is
// depends on something the track cannot say, so the panel is what decides how
// it reads.
export const isUnchecked = (track: ListedEntry) =>
	!!track.permalink && entriesFor(track).length === 0 && !isWithoutLinks(track);

export const uncheckedTracks = (tracks: ListedEntry[]) => tracks.filter(isUnchecked);

/**
 * Every listed track once, in the order the list holds them, with all of its
 * entries; a track with no links has none.
 *
 * Every track, unconditionally — which is the difference a kept list makes.
 * The old page-shaped view left out anything it had no answer about, since on
 * a page that meant a row that was still on its way; here it would mean hiding
 * a track somebody put on the list on purpose. `checking` is the set of
 * permalinks a lookup is running for right now, so those rows can say they are
 * on their way rather than that nobody has asked.
 */
export function buildListOrder(tracks: ListedEntry[], { checking = new Set() }: { checking?: Set<string> } = {}) {
	return tracks.map((track) => ({
		track,
		entries: entriesFor(track),
		pending: isUnchecked(track) && checking.has(track.permalink)
	}));
}

// What the search box matches a row against: title, artist, the sites its
// links point to, and the page it was added from — so typing a playlist's name
// narrows the list to what came in from it.
export const searchText = (track: ListedEntry, entries: LinkEntry[]) =>
	[track.title, track.artist, track.from, ...entries.map((entry) => entry.host)].join(' ');

/** Tracks whose details could not be fetched, fully or at all. */
export function lookupNotes({
	tracks,
	lookupFailed,
	unseen
}: Pick<DetailsAnswer, 'tracks' | 'lookupFailed' | 'unseen'>) {
	const notes = [];
	const unknown = tracks.filter((t) => !t.details).length;

	if (unknown > 0) {
		// `unseen` means the lookup stopped rather than asking about each of them
		// in turn: nothing it had already asked about came back, which is what a
		// page the anonymous API cannot describe looks like. Saying "could not be
		// checked" there would be true but useless — this says why, and that the
		// page itself is still the source for whatever it renders.
		const some = `couldn’t check ${unknown} track${unknown === 1 ? '' : 's'}`;
		notes.push(
			unseen
				? `${some}: SoundCloud won’t describe them to a signed-out visitor — usually that means ` +
						'they’re private. what the page shows for them is still listed.'
				: some + (lookupFailed > 0 ? ' — SoundCloud didn’t answer for them.' : '.')
		);
	}

	// Only what the page shows: the purchase link, but nothing about a
	// SoundCloud download.
	const partial = tracks.filter((t) => t.details && t.details.downloadable === null).length;

	if (partial > 0) {
		notes.push(
			`${partial} track${partial === 1 ? '' : 's'}: purchase link read off the page, SoundCloud download unknown.`
		);
	}

	return notes.join(' ');
}

/** The lookup failures that leave nothing to show at all. '' is no failure, and has no sentence. */
export const LOOKUP_ERRORS: Partial<Record<LookupError, string>> = {
	'no-client-id': 'couldn’t find SoundCloud’s API key in the page — reload the tab.',
	unauthorized: 'SoundCloud rejected the lookups (the page’s API key has expired) — reload the tab.'
};

// ---- What a row says when it is opened --------------------------------------
//
// A row is a thumbnail, a title and three or four buttons, and there is no
// room on it for anything else — but a track is described by more than that,
// and most of it is worth having as text somewhere else (a spreadsheet, a
// message, a search box). So a row opens into the rest of what is known about
// it, one field a line, each of them one click away from the clipboard.
//
// Where each field comes from, and why it can be unknown:
//
//   title, artist   the page's own row. Always there.
//   genre           the page's own tag beside the row, where it rendered one
//                   (Likes rows and track pages do; playlist rows and the
//                   badge tiles do not), else the lookup (SoundCloud's API).
//                   '' from the lookup means the uploader set none; null or
//                   missing on both means nobody has learned it. (Details
//                   cached by a version from before the genre was kept do not
//                   linger as unknowns: content.js counts such an entry stale
//                   and asks again.)
//   from            the list itself: the page this track was added from. The
//                   one field that is about the list rather than the track,
//                   and the reason it is worth a line — a list built from six
//                   playlists over a fortnight otherwise says nothing about
//                   where any of it came from.
//
// The link is the track's own address, which is the one field that is never
// unknown — a track with no permalink is not listed at all.
//
// Liked and reposted are not among them. They are a yes or a no, not words
// anybody pastes anywhere, and two lines reading "not liked" / "not reposted"
// took a third of the opened row to say nothing happened. They are drawn as a
// pair of small icons instead — see `rowMarks`.

// Built the same way `entriesFor` builds a SoundCloud entry's URL, and with
// the same assumption behind it: a track with no permalink is never listed, so
// there is no such row to draw this for.
export const trackUrl = (permalink: string) => `https://soundcloud.com${permalink}`;

// ---- The two marks an opened row draws rather than spells out ---------------
//
// Both come from the row's own buttons, read at extraction time, and both have
// three states rather than two: `null` is "the row rendered no such button"
// (the badge tiles render none), which is not the same as "no". Saying "not
// liked" to somebody whose like the page never drew would be a claim this
// extension cannot make, so the unknown state is drawn as its own thing and
// says as much when you point at it.
/** `hint` is keyed by `String(state)`: 'true', 'false' or 'null'. */
const MARKS: {
	id: 'liked' | 'reposted';
	yes: string;
	no: string;
	hint: Record<string, string>;
}[] = [
	{
		id: 'liked',
		yes: 'liked',
		no: 'not liked',
		hint: {
			true: 'you’ve liked this on SoundCloud.',
			false: 'you haven’t liked this.',
			null: 'the row this came off drew no like button.'
		}
	},
	{
		id: 'reposted',
		yes: 'reposted',
		no: 'not reposted',
		hint: {
			true: 'you’ve reposted this on SoundCloud.',
			false: 'you haven’t reposted this.',
			null: 'the row this came off drew no repost button.'
		}
	}
];

/**
 * The marks an opened row draws, in the order it draws them.
 *
 * Each is `{ id, state, label, hint }`. `state` is `true`, `false` or `null`
 * for unknown — three states, three ways of drawing it. `label` is what a
 * screen reader reads, and follows the dead field's wording for a thing
 * nobody has an answer for ("liked: unknown").
 */
export const rowMarks = (track: ListedEntry) =>
	MARKS.map(({ id, yes, no, hint }) => {
		const state = track[id] ?? null;

		return {
			id,
			state,
			label: state === true ? yes : state === false ? no : `${id}: unknown`,
			hint: hint[String(state)]
		};
	});

/**
 * The fields an opened row shows, in the order it shows them.
 *
 * Each is `{ id, label, value, copy }`. `value` is the words on screen;
 * `copy` is what a click puts on the clipboard, and '' means there is nothing
 * to copy — the field is standing in for an answer rather than being one, and
 * its button is dead.
 *
 * `titleCased` is the setting of the same name ($lib/title-case.svelte.js),
 * and applies to the three fields that are words — the title, the artist and
 * the genre. Not to the link, which is an address and case is meaning in one,
 * and not to "from", which is this list's own sentence about where a track
 * came in rather than anything SoundCloud said. It is applied to what the
 * field *says*, so what a click copies is what is on screen: somebody who
 * asked for a tracklist in Title Case asked for it in the document too, and a
 * field that copied something other than its own words would be a trap.
 *
 * @param track a listed track
 */
export function rowFacts(track: ListedEntry, { titleCased = false }: { titleCased?: boolean } = {}) {
	const cased: (value: string) => string = titleCased ? titleCase : (value) => value;
	// The page first, the lookup second. Both name the same thing — the tag
	// SoundCloud renders beside a row *is* the track's genre field — but the
	// page's answer is there the moment the track is added, needs no request,
	// and is the only one a track that came in through the session queue has.
	const genre = track.genre || track.details?.genre;

	const fields = [
		// A page that rendered no artist gives '', which reads as "unknown" like
		// any other empty field: it is what this extension could find out, and
		// pasting a blank into a spreadsheet is not an answer either.
		{ id: 'title', label: 'title', value: cased(track.title) },
		{ id: 'artist', label: 'artist', value: cased(track.artist) },
		{
			id: 'genre',
			label: 'genre',
			// '' from the API is an answer — the uploader filed it under nothing —
			// and one worth showing as such, since it is the reason the field is
			// empty rather than a gap in what was asked.
			value: cased(genre || ''),
			unknown: genre == null ? 'unknown' : genre === '' ? 'none' : ''
		},
		{ id: 'link', label: 'link', value: trackUrl(track.permalink) },
		// Empty for a track put on the list by a version of this tool that did
		// not record it, or one whose source was never named; "unknown" then, as
		// any other empty field.
		{ id: 'from', label: 'from', value: track.from || '' }
	];

	return fields.map(({ id, label, value, unknown }) => ({
		id,
		label,
		value: value || unknown || 'unknown',
		// Only a real value goes on the clipboard: "unknown" is this panel
		// talking about itself, and pasting it into a spreadsheet would put a
		// word there that no one said about the track.
		copy: value
	}));
}

/**
 * What a download button says it is, as its accessible name — the same
 * sentence wherever the link is shown, and never the uploader's own wording
 * for it (which is a label on a page, not a description of a button).
 */
export const downloadLabel = (entry: LinkEntry) =>
	entry.kind === 'soundcloud'
		? `open the track page to download${entry.spent ? ' (no downloads left)' : ''}`
		: `download from ${entry.host}`;
