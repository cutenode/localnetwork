// The artists tool's list: the uploaders behind the tracks somebody has
// added, kept for good.
//
// The same arrangement as the Manager's list (src/tools/manager/listed.svelte.ts)
// and for the same reasons: nothing goes onto it by itself, what is on it
// stays across openings of the popup, tabs and browser restarts until it is
// taken off, and it lives in chrome.storage.local beside the marks and the
// details cache — what the tool knows, as against where somebody is in a
// piece of work, which is the session's. The exported state file carries it;
// state-transfer.ts reads this key directly, and test/consistency.test.ts
// checks the two agree on it.
//
// What an entry is: an artist as the panel lists one — the uploader's path,
// which is its identity; the name a page gave; the tracks that brought it in,
// so the row can say why it is here — plus what SoundCloud said about the
// account when it was asked (`profile`, see content.ts GET_ARTIST_PROFILES),
// and the two things the list itself knows: where the artist came in from,
// and when.

import { isArtistPath } from './profiles.ts';
import { storedList } from '$lib/stored';
import type { ArtistEntry, ProfileAnswer, ProfileEntry, SavedArtistEntry } from '$lib/types';
import { fallback, z } from '$lib/zod';

/** The storage key. state-transfer.js writes the same one; keep them in step. */
export const ARTISTS_KEY = 'artists';

// How many artists the list holds. An entry with a profile is a kilobyte or
// so — the bio is the bulk of it, and content.js caps that — so two thousand
// of them is a couple of megabytes of the 10 MB area, beside the Manager's
// list and the details cache. Past it the list refuses rather than dropping
// the oldest, for the reason the Manager's does: these were put here on
// purpose, and the add says how many did not fit.
export const MAX = 2000;

// How many of an artist's tracks an entry remembers. Enough to say "12
// tracks" and to name them when the row is opened; a label whose every
// upload is on the list does not need all four hundred spelled out here.
export const TRACKS_MAX = 50;

// How soon after a profile arrives the list is written back. Answers come in
// a few at a time while a lookup runs, and a write per answer would be a write
// of the whole list every few hundred milliseconds; a second's coalescing
// makes it one per burst. The panel flushes it when the lookup ends.
export const SAVE_DELAY_MS = 1000;

const Text = fallback(z.string(), '');

/** The tracks an artist is on the list for, as `{ permalink, title }`. */
const tracksOf = storedList(z.object({ permalink: z.string().check(z.minLength(1)), title: Text }));

/** Any object at all, which is as far as a stored profile is checked. */
const AnObject = z.looseObject({});

/** What SoundCloud said, a remembered failure, or null for nobody asked. */
const Profile = fallback(z.nullable(z.custom<ProfileEntry>((value) => AnObject.safeParse(value).success)), null);

/** Where an artist came in from, where it carries one already. */
const From = fallback(z.optional(z.string()), undefined);

/**
 * The fields of an artist the list keeps.
 *
 * `from` and `at` are the list's own two, given here for an artist a page
 * just handed over; an artist that carries them already was on a list
 * before (an entry out of an imported state file) and keeps its own.
 *
 * @param artist as artistsOf() shapes one, or an entry out of a file
 */
export const entryOf = (artist: SavedArtistEntry, { from = '', at = Date.now() } = {}): ArtistEntry => ({
	permalink: artist.permalink,
	name: Text.parse(artist.name),
	tracks: tracksOf(artist.tracks).slice(0, TRACKS_MAX),
	profile: Profile.parse(artist.profile),
	/** The page (or the list, or the queue) this artist was added from. */
	from: From.parse(artist.from) ?? from,
	/** When it was added, in milliseconds. */
	at: Number.isInteger(artist.at) ? artist.at! : at
});

/**
 * What is in storage, as a list, dropping anything that is not an entry.
 * An entry without an artist's path is not an entry: there would be nothing
 * to look up, nothing to tell it from another, and nothing to remove it by.
 */
const ArtistPathed = z.looseObject({ permalink: z.string().check(z.refine(isArtistPath)) });

export const listOf = storedList(z.custom<ArtistEntry>((item) => ArtistPathed.safeParse(item).success));

// Which of two answers about an artist to keep. A profile beats a remembered
// failure beats nobody having asked; between two of a kind the newer wins,
// which is what makes looking an artist up again a refresh.
export function betterProfile(held: ProfileEntry | null, fresh: ProfileEntry | null) {
	if (!fresh) return held;

	if (!held) return fresh;

	if (held.unavailable && !fresh.unavailable) return fresh;

	if (fresh.unavailable && !held.unavailable) return held;

	return (fresh.at ?? 0) >= (held.at ?? 0) ? fresh : held;
}

// What an entry becomes when the same artist is added again: the fresh name
// where the page gave one, the tracks of both (the ones already here first),
// the better of the two answers, and the *first* time and source.
const refresh = (held: ArtistEntry, fresh: ArtistEntry) => {
	const known = new Set(held.tracks.map((one) => one.permalink));
	const tracks = [...held.tracks, ...fresh.tracks.filter((one) => !known.has(one.permalink))];

	return {
		...fresh,
		name: fresh.name || held.name,
		tracks: tracks.slice(0, TRACKS_MAX),
		profile: betterProfile(held.profile, fresh.profile),
		from: held.from,
		at: held.at
	};
};

class Artists {
	/** In the order they were added. */
	#items: ArtistEntry[] = $state([]);

	#started = false;

	#saveTimer: ReturnType<typeof setTimeout> | null = null;

	get items() {
		return this.#items;
	}

	get count() {
		return this.#items.length;
	}

	/** The list as plain data, for the state file and for storage writes. */
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
			const stored = await chrome.storage.local.get(ARTISTS_KEY);
			this.#items = listOf(stored[ARTISTS_KEY]);
		} catch (_e) {
			// No storage to read (or none granted): the list starts empty and is
			// held in memory for as long as this popup is open.
			return;
		}

		// Another window's popup, or an import, can move it under us.
		chrome.storage.onChanged.addListener((changes, area) => {
			if (area === 'local' && changes[ARTISTS_KEY]) {
				this.#items = listOf(changes[ARTISTS_KEY].newValue);
			}
		});
	}

	/**
	 * Puts artists on the list, in one write however many there are.
	 *
	 * An artist already on it is not added twice: the entry takes the fresh
	 * name and the new tracks instead, and the better of the two answers.
	 *
	 * @param artists as artistsOf() shapes them, or entries out of a file
	 * @param options what to file the new ones under
	 */
	async add(artists: SavedArtistEntry[], { from = '' } = {}) {
		const at = Date.now();
		// Plain Map and Set on purpose: local to this call, never read through
		// the markup, and a reactive one would cost a proxy per write.
		const held = new Map(this.#items.map((item) => [item.permalink, item]));
		const seen = new Set();
		let added = 0;
		let refreshed = 0;
		let refused = 0;

		for (const artist of artists) {
			if (!artist || !isArtistPath(artist.permalink)) continue;

			if (seen.has(artist.permalink)) continue;
			seen.add(artist.permalink);
			const fresh = entryOf(artist, { from, at });
			const known = held.get(artist.permalink);

			if (known) {
				held.set(artist.permalink, refresh(known, fresh));
				refreshed += 1;
				continue;
			}

			if (held.size >= MAX) {
				refused += 1;
				continue;
			}

			held.set(artist.permalink, fresh);
			added += 1;
		}

		if (added > 0 || refreshed > 0) await this.#write([...held.values()]);

		return { added, refreshed, refused };
	}

	/**
	 * Makes the list exactly these artists, for an import asked to replace
	 * what is here (state-transfer.js). The file's order is the list's; an
	 * artist the file names twice lands once; one past MAX is refused.
	 *
	 * @returns `added` is how many the list holds now
	 */
	async replace(artists: SavedArtistEntry[]) {
		const at = Date.now();
		const held: Map<string, ArtistEntry> = new Map();
		let refused = 0;

		for (const artist of artists) {
			if (!artist || !isArtistPath(artist.permalink)) continue;
			const fresh = entryOf(artist, { at });
			const known = held.get(artist.permalink);

			if (known) {
				held.set(artist.permalink, refresh(known, fresh));
				continue;
			}

			if (held.size >= MAX) {
				refused += 1;
				continue;
			}

			held.set(artist.permalink, fresh);
		}

		await this.#write([...held.values()]);

		return { added: held.size, refused };
	}

	/**
	 * Takes in what a lookup said: a profile per artist, or that there is
	 * none (`{ permalink, unavailable: true }`). Answers for artists not on
	 * the list are dropped — they may have been taken off while the lookup
	 * ran. Each is stamped with the time it arrived.
	 *
	 * Written back a moment later rather than at once (see SAVE_DELAY_MS);
	 * `flush()` writes it now.
	 *
	 * @returns how many entries took an answer
	 */
	learn(profiles: ProfileAnswer[]) {
		let taken = 0;
		const at = Date.now();
		const answers: Map<string, ProfileEntry> = new Map();

		for (const profile of Array.isArray(profiles) ? profiles : []) {
			if (!profile || !isArtistPath(profile.permalink)) continue;
			answers.set(profile.permalink, profile.unavailable ? { unavailable: true, at } : { ...profile, at });
		}

		if (answers.size === 0) return 0;
		this.#items = this.#items.map((item) => {
			const answer = answers.get(item.permalink);

			if (!answer) return item;
			taken += 1;

			return { ...item, profile: answer };
		});

		if (taken > 0) this.#scheduleSave();

		return taken;
	}

	/** Writes what `learn` has taken in, if a write is still pending. */
	flush() {
		if (this.#saveTimer === null) return Promise.resolve();
		clearTimeout(this.#saveTimer);
		this.#saveTimer = null;

		return this.#write(this.#items);
	}

	/** Takes a whole set of artists off the list in one write. */
	removeMany(permalinks: string[]) {
		const gone = new Set(permalinks);

		return this.#write(this.#items.filter((item) => !gone.has(item.permalink)));
	}

	#scheduleSave() {
		if (this.#saveTimer !== null) return;
		this.#saveTimer = setTimeout(() => {
			this.#saveTimer = null;
			void this.#write(this.#items);
		}, SAVE_DELAY_MS);
	}

	// The list on screen is the list in memory: it is written back on every
	// change, and a write that cannot happen costs it only its life beyond this
	// popup.
	async #write(items: ArtistEntry[]) {
		this.#items = items;

		try {
			await chrome.storage.local.set({ [ARTISTS_KEY]: $state.snapshot(items) });
		} catch (_e) {
			// Nothing to do: the panel already shows what it holds.
		}
	}
}

export const artists = new Artists();
