// The Manager's list: the tracks somebody has put on it, kept for good.
//
// The tool used to *be* the page in front of it. Opening it read the active
// tab, asked SoundCloud about every track that tab rendered, and drew the
// answer; closing the popup threw the lot away, and opening it over another
// tab drew another page. Two things were wrong with that. Nothing could be
// worked on unless its page was the one in front — so the panel had nothing
// at all to say on a Hypeddit tab, which is exactly where one ends up while
// working through a list — and opening the tool was itself a few hundred
// requests to SoundCloud about tracks nobody had asked about, paid again on
// every opening.
//
// So the list is its own thing now, and the page is a source you add from.
// Nothing enters it until somebody presses **add**; what is on it stays on it
// across openings of the popup, across tabs and across browser restarts, until
// it is taken off.
//
// Where it lives: chrome.storage.local, under the key below, beside the marks
// (downloaded-store.ts) and the details cache (content.ts) and for the same
// reason — those are what the tool knows, as against where somebody is in a
// piece of work, which is the session's (view.svelte.ts) and dies with the
// browser. The exported state file carries all three; state-transfer.ts reads
// this key directly, and test/consistency.test.ts checks the two agree on it.
//
// What an entry is: a track as the panel lists it — enough to draw the row, to
// fetch the artwork, to find the download links and to open it out — plus the
// two things the list itself knows, which the page never said: where the track
// came in from, and when.

import { storedList, storedTrack } from '$lib/stored';
import type { ListedEntry, SavedListedEntry, Track, TrackDetails } from '$lib/types';

/** A track from any hop, or an entry out of a state file, which carries the list's own two. */
type Listable = Track & Partial<Pick<ListedEntry, 'from' | 'at'>>;

/** The storage key. state-transfer.js writes the same one; keep them in step. */
export const LISTED_KEY = 'listed';

// How many tracks the list holds. chrome.storage.local is 10 MB without the
// unlimitedStorage permission, which this extension does not ask for; an entry
// is a few hundred bytes, so five thousand of them is a small fraction of it
// and leaves the details cache — which is far bigger, and shared with the
// content script — the room it needs.
//
// Past it the list *refuses* rather than dropping the oldest, which is what a
// cache would do. These are tracks somebody put here on purpose, and silently
// losing the first playlist of the day to the fourth is not a trade this gets
// to make on their behalf: the add says how many went on and how many did not,
// and taking some off is one selection away.
export const MAX = 5000;

/**
 * The fields of a track the list keeps.
 *
 * `from` and `at` are the list's own two — where a track came in from, and
 * when — and a page never carries either, so for a page they are the ones
 * given here. A track that *does* carry them is one that was on a list
 * already: an entry out of an imported state file. Its own answers are the
 * true ones, and taking them rather than stamping "just now, from nowhere"
 * is what makes a restored list the list that was exported.
 *
 * @param stamp what to file a track under that carries neither.
 */
export const entryOf = (
	track: Listable,
	{ from = '', at = Date.now() }: { from?: string; at?: number } = {}
): ListedEntry => ({
	permalink: track.permalink,
	title: track.title ?? '',
	artist: track.artist ?? '',
	artworkUrl: track.artworkUrl ?? '',
	details: track.details ?? null,
	// What the page said about the track and nothing else did: the genre it
	// rendered beside the row, and whether the row's own like and repost
	// buttons were on. `null` is "the page never said", which is not the same
	// as "no genre" or "not liked" — see rowFacts.
	genre: track.genre ?? null,
	liked: track.liked ?? null,
	reposted: track.reposted ?? null,
	// The uploader's own wording for a purchase link, where the details do not
	// carry it; entriesFor falls back to it for the button's tooltip.
	domPurchaseTitle: track.domPurchaseTitle ?? '',
	/** The page (or the queue) this track was added from. */
	from: track.from ?? from,
	/** When it was added, in milliseconds. */
	at: Number.isInteger(track.at) ? track.at! : at
});

/**
 * What is in storage, as a list — dropping anything that is not an entry.
 *
 * Storage is not this version's to trust: a list written by another one, or
 * restored from an imported state file, can hold shapes nothing here expects.
 * An entry without a permalink is not an entry at all — there would be nothing
 * to tell it from another, nothing to mark when its turn came and nothing to
 * remove it by — which is the same bar the session queue sets.
 *
 * @returns taken on trust past the permalink.
 */
export const listOf = storedList(storedTrack<ListedEntry>());

// Which of two answers about a track to keep, when a page is added that
// already has tracks on the list.
//
// Details come in three strengths. `null` is nobody has asked. A *partial* one
// is what a Likes row renders by itself — the purchase link, and nulls where
// SoundCloud's own download would be said — so it is an answer about one half
// of the question. A full one has been asked of SoundCloud and answers both.
// A stronger answer always wins; between two of the same strength the newer
// does, which is what makes adding a page again a refresh rather than a no-op.
/** @param fresh undefined where no answer has arrived. */
export function betterDetails(held: TrackDetails | null, fresh: TrackDetails | null | undefined): TrackDetails | null {
	if (!fresh) return held;

	if (!held) return fresh;

	if (held.partial && !fresh.partial) return fresh;

	if (fresh.partial && !held.partial) return held;

	return fresh;
}

// What an entry becomes when the same track is added again: the fresh row's
// words, and its picture where it showed one (the page may have been renamed,
// the artwork re-uploaded), the better of the two answers, and the *first*
// time and source — when a track came onto the list is not something a second
// sighting gets to rewrite.
const refresh = (held: ListedEntry, fresh: ListedEntry): ListedEntry => ({
	...fresh,
	// `''` is "the row never showed one", not "there is none" — a page read
	// without scrolling through all of it renders no thumbnail for most of its
	// rows, and adding it again must not take the cover off the tracks that
	// have one. The same reading `genre`, `liked` and `reposted` get below: a
	// layout that renders none of them (the badge tiles) must not wipe what a
	// layout that does had already answered.
	artworkUrl: fresh.artworkUrl || held.artworkUrl,
	details: betterDetails(held.details, fresh.details),
	genre: fresh.genre ?? held.genre,
	liked: fresh.liked ?? held.liked,
	reposted: fresh.reposted ?? held.reposted,
	from: held.from,
	at: held.at
});

class Listed {
	/** In the order they were added. */
	#items = $state<ListedEntry[]>([]);

	#started = false;

	get items() {
		return this.#items;
	}

	get count() {
		return this.#items.length;
	}

	/**
	 * The list as plain data, for anything outside the popup's own reactivity —
	 * the exported state file, and the structured clone every storage write goes
	 * through, neither of which will take a `$state` proxy.
	 */
	get entries() {
		return $state.snapshot(this.#items);
	}

	/** The pages the list was built from, in the order they were first added. */
	get sources() {
		return [...new Set(this.#items.map((item) => item.from).filter(Boolean))];
	}

	has(permalink: string) {
		return this.#items.some((item) => item.permalink === permalink);
	}

	/** Reads the list and keeps following it. Safe to call more than once. */
	async start() {
		if (this.#started) return;
		this.#started = true;

		try {
			const stored = await chrome.storage.local.get(LISTED_KEY);
			this.#items = listOf(stored[LISTED_KEY]);
		} catch (_e) {
			// No storage to read (or none granted): the list starts empty and is
			// held in memory for as long as this popup is open.
			return;
		}

		// Another window's popup, or an import, can move it under us.
		chrome.storage.onChanged.addListener((changes, area) => {
			if (area === 'local' && changes[LISTED_KEY]) {
				this.#items = listOf(changes[LISTED_KEY].newValue);
			}
		});
	}

	/**
	 * Puts tracks on the list, in one write however many there are.
	 *
	 * A track already on it is not added twice: its entry takes the better of
	 * the two answers instead, so adding a page again is how a list is brought
	 * up to date rather than how it grows a duplicate of itself.
	 *
	 * @param tracks a page's, the queue's or an imported file's.
	 * @param options what to file the new ones under.
	 * @returns how many were new, how many were already here, and how many did not fit.
	 */
	async add(tracks: Listable[], { from = '' }: { from?: string } = {}) {
		const at = Date.now();
		// Plain Map and Set on purpose: both are local to this call, nothing ever
		// reads them back through the markup, and a reactive one would only cost a
		// proxy on every write while a page of five hundred is being folded in.
		// eslint-disable-next-line svelte/prefer-svelte-reactivity
		const held = new Map(this.#items.map((item) => [item.permalink, item]));
		// eslint-disable-next-line svelte/prefer-svelte-reactivity
		const seen = new Set();
		let added = 0;
		let refreshed = 0;
		let refused = 0;

		for (const track of tracks) {
			if (!track || !track.permalink) continue;

			// A page can list the same track twice (a playlist with a duplicate in
			// it); the second sighting is the same track, so it refreshes the first
			// rather than counting as anything.
			if (seen.has(track.permalink)) continue;
			seen.add(track.permalink);
			const fresh = entryOf(track, { from, at });
			const known = held.get(track.permalink);

			if (known) {
				held.set(track.permalink, refresh(known, fresh));
				refreshed += 1;
				continue;
			}

			if (held.size >= MAX) {
				refused += 1;
				continue;
			}

			held.set(track.permalink, fresh);
			added += 1;
		}

		if (added > 0 || refreshed > 0) await this.#write([...held.values()]);

		return { added, refreshed, refused };
	}

	/**
	 * Makes the list exactly these tracks, for an import asked to replace what
	 * is here rather than add to it (state-transfer.js). Whatever was listed is
	 * gone, and the file's own order is the list's order; a track the file names
	 * twice still lands once, and a file longer than the list can hold is cut
	 * off at MAX rather than allowed to overflow storage.
	 *
	 * @returns how many are on it afterwards, and how many did not fit.
	 */
	async replace(tracks: SavedListedEntry[]) {
		const at = Date.now();
		// eslint-disable-next-line svelte/prefer-svelte-reactivity
		const held = new Map<string, ListedEntry>();
		let refused = 0;

		for (const track of tracks) {
			if (!track || !track.permalink) continue;
			const fresh = entryOf(track, { at });
			const known = held.get(track.permalink);

			if (known) {
				held.set(track.permalink, refresh(known, fresh));
				continue;
			}

			if (held.size >= MAX) {
				refused += 1;
				continue;
			}

			held.set(track.permalink, fresh);
		}

		await this.#write([...held.values()]);

		return { added: held.size, refused };
	}

	/** Takes a whole set of tracks off the list in one write. */
	removeMany(permalinks: string[]) {
		const gone = new Set(permalinks);

		return this.#write(this.#items.filter((item) => !gone.has(item.permalink)));
	}

	// The list on screen is the list in memory: it is written back on every
	// change, and a write that cannot happen costs it only its life beyond this
	// popup — which is still better than a panel that shows one thing and holds
	// another.
	async #write(items: ListedEntry[]) {
		this.#items = items;

		try {
			await chrome.storage.local.set({ [LISTED_KEY]: $state.snapshot(items) });
		} catch (_e) {
			// Nothing to do: the panel already shows what it holds.
		}
	}
}

export const listed = new Listed();
