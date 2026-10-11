// The session queue: the tracks picked out while working through a page,
// held together so a whole batch of them can be dealt with at once.
//
// Opening a download link used to mark its track as downloaded on the spot,
// which said something that was not known yet — the file had not been saved,
// and often the gate behind the link had not even been walked. A track goes
// into this queue instead: the queue sheet marks the lot as downloaded in one
// go, and two of the three tools can work from it rather than from the page
// (its gates, its cover art, its titles).
//
// Where it is shown: the header of every view carries the count and opens the
// sheet over whatever is there (QueueButton.svelte, QueueSheet.svelte). It
// was a tool of its own until then, which meant the one thing that is *about*
// the other tools could only be looked at by leaving them.
//
// Where it lives: chrome.storage.session, which is the extension's own storage
// for the length of the browsing session — it survives the popup closing (it
// closes on every click that opens a tab) and going back and forth between
// tools, and is gone when the browser is. That is the whole point of a
// *session* queue: it is a working list, not a library. The marks it makes are
// what lasts (downloaded-store.ts).
//
// Reading it is a round trip to the browser process, and everything the queue
// is behind waits on that: the count in the header of every view, the sheet's
// rows, and — because App.svelte fetches the sheet's chunk as soon as there is
// something in the queue worth looking at — the fetch of the chunk itself. A
// browser-action popup is destroyed on every click that takes the focus off
// it, which in these tools is most clicks, so that was a round trip of empty
// header on every single opening, for a queue that had not moved since the
// last one.
//
// So the queue is mirrored into the popup's own localStorage, which is
// *synchronous* — the same thing view.svelte.ts does with the tool that was
// open, for the same reason. `items` answers with the mirror on the first line
// the popup runs, and the session read corrects it a moment later. What the
// mirror cannot know is when a browsing session ended, since localStorage
// outlives one, or what was queued from a SoundCloud page while the popup was
// shut; session storage settles both, and `start` is where it does. In the
// window before it answers the mirror is a guess, which is worth saying
// plainly: a track queued twice in that window is refused the second time as
// one already held. It is a few milliseconds against a blank header on every
// opening, and the trade is the one already made for the route.
//
// An entry is a track, with only the fields the tools that consume the queue
// need: enough to list it, to fetch its artwork, to find its download gate and
// to mark it. Panels read `items` as they would a page's `tracks`.

import type { QueueEntry, Track } from '$lib/types';
import { storedList, storedTrack } from './stored.ts';

const KEY = 'queue';

// A content script is an untrusted context, and chrome.storage.session is shut
// to those by default — so the queue would be invisible on the SoundCloud page
// itself, where the "Queued" pills are painted (content.js). Opening the area
// is a call only a trusted context may make, and this popup is the one that
// has it: it is made once, when the queue starts, before anything is written.
//
// Nothing sensitive is on the other side of that door — the queue is a list of
// permalinks the user picked out on the page it is shown on — and the page's
// own scripts still cannot reach it: "untrusted" here means the extension's
// content script, not SoundCloud's JavaScript.
//
// It is a round trip to the browser process, and the read of the queue is
// another; the read does not depend on it, so the two go at once and only a
// *write* waits for the door to be open (see `#opened` and `#write`).
async function openToContentScripts() {
	try {
		await chrome.storage.session.setAccessLevel({
			accessLevel: 'TRUSTED_AND_UNTRUSTED_CONTEXTS'
		});
	} catch (_e) {
		// An older browser, or no session storage at all: the queue still works
		// here, the page just paints no queued pills.
	}
}

/**
 * The fields of a track a queued entry keeps.
 *
 * The last three are what only the page it was queued from could say: the
 * genre it rendered beside the row, and whether the row's own like and repost
 * buttons were on. Nothing in the queue itself shows them — a queued track is
 * a line with a cover and a title — but the queue is a source the Manager's
 * list adds from, and a track that reached the list this way used to arrive
 * knowing none of the three, for good: the page it came off is long gone by
 * then, and no lookup answers them. So they are carried through rather than
 * dropped here. `null` (or absent, in a queue written by an older version) is
 * "the page never said", which the panel shows as unknown.
 */
export const entryOf = ({
	permalink,
	title,
	artist,
	artworkUrl,
	details,
	genre = null,
	liked = null,
	reposted = null
}: Track): QueueEntry => ({
	permalink,
	title,
	artist,
	artworkUrl,
	details,
	genre,
	liked,
	reposted
});

/**
 * What is in storage, as a queue — dropping anything that is not an entry.
 *
 * Session storage is not this version's to trust: a queue written by another
 * one, or restored from an imported state file, can hold shapes nothing here
 * expects, and an entry without a permalink is not an entry at all — there
 * would be nothing to tell it from another and nothing to mark when its turn
 * came (`add` refuses one for the same reason).
 *
 * It matters more than it reads: the queue is behind the count in the header
 * of every view now, so one bad entry would not cost a tool, it would cost
 * the popup.
 *
 * @returns only the permalink is checked; the rest is taken as it was written
 */
const queueOf = storedList(storedTrack<QueueEntry>());

// Where the mirror lives: the popup's own localStorage, under a key of its
// own. Not a `pref:` key — it is not a setting, and it is not in the exported
// state file (state-transfer.js lists what is, and the queue goes into that
// through the store rather than through any key).
const MIRROR = 'queue:items';

/** What the mirror says was queued, read before anything else runs. */
const mirrored = () => {
	try {
		return queueOf(JSON.parse(localStorage.getItem(MIRROR) ?? 'null'));
	} catch (_e) {
		// No storage to read, or something that is not JSON in it: the popup
		// waits for the session, which is where it waited before there was a
		// mirror.
		return [];
	}
};

/** Drops the mirror, for when there is nothing true to put in it. */
const forget = () => {
	try {
		localStorage.removeItem(MIRROR);
	} catch (_e) {
		// No storage at all, so nothing stale in it either.
	}
};

/**
 * Puts the queue in the mirror, for the next opening to draw from.
 *
 * `JSON.stringify` reads through a `$state` proxy quite happily — it is
 * `structuredClone`, which the storage write goes through, that refuses one —
 * so this takes the items as they are.
 */
const remember = (items: QueueEntry[]) => {
	if (items.length === 0) return forget();

	try {
		localStorage.setItem(MIRROR, JSON.stringify(items));
	} catch (_e) {
		// No storage to write, or no room left in it. A mirror that could not be
		// brought up to date is worse than none: it would paint a queue that has
		// moved on. So what is there goes, and the next opening waits.
		forget();
	}
};

class Queue {
	/**
	 * In the order they were added.
	 *
	 * The mirror to start with, so the header has a count and the sheet has its
	 * rows on the first turn; `start` replaces it with what the session really
	 * holds.
	 */
	#items: QueueEntry[] = $state(mirrored());

	#started = false;

	/** Whether anything has been queued or unqueued since the popup opened. */
	#written = false;

	/** The access-level call, once it is under way. */
	#opened: Promise<void> | null = null;

	get items() {
		return this.#items;
	}

	get count() {
		return this.#items.length;
	}

	get permalinks() {
		return this.#items.map((item) => item.permalink);
	}

	/**
	 * The queue as plain data, for anything outside the popup's own reactivity.
	 * The exported state file is written from this rather than from session
	 * storage (state-transfer.js), because a queue that could not be written
	 * there is still a queue: it is held here and shown on screen, and a backup
	 * that leaves out what is on screen is the one thing a backup must not do.
	 *
	 * `$state.snapshot` is what makes it plain, for the same reason `#write`
	 * calls it: what `$state` holds are proxies, and neither `structuredClone`
	 * nor a comparison against plain data will take one.
	 */
	get entries() {
		return $state.snapshot(this.#items);
	}

	has(permalink: string) {
		return this.#items.some((item) => item.permalink === permalink);
	}

	/**
	 * Reads the queue and keeps following it. Safe to call more than once.
	 *
	 * This is where the mirror stops being a guess. What comes back replaces it
	 * whole rather than being merged into it: the session *is* the queue, and
	 * the mirror is a copy of some earlier moment of one — a browsing session
	 * that has since ended, or one this popup was shut through.
	 */
	async start() {
		if (this.#started) return;
		this.#started = true;
		this.#opened = openToContentScripts();
		let session: QueueEntry[] = [];

		try {
			session = queueOf((await chrome.storage.session.get(KEY))[KEY]);
		} catch (_e) {
			// No session storage to read: the queue is empty and is kept in memory
			// for as long as this popup is open. The mirror goes with it — it is a
			// guess about a session, and there is no session to guess at.
			this.#settle(session);

			return;
		}

		this.#settle(session);
		chrome.storage.onChanged.addListener((changes, area) => {
			// Another window's popup, or the queue button on a SoundCloud page.
			if (area === 'session' && changes[KEY]) this.#hold(queueOf(changes[KEY].newValue));
		});
	}

	// What the session held, unless this popup has since said otherwise.
	//
	// The read is a round trip, and the mirror is there so that something can
	// be queued before it lands; a track picked out in that window is newer
	// than the answer on its way back, and `#write` has already put the queue
	// it belongs to in the session. Taking the older answer would undo it on
	// screen while leaving it written, which is the one outcome worse than
	// either.
	#settle(items: QueueEntry[]) {
		if (!this.#written) this.#hold(items);
	}

	/**
	 * Queues the tracks that are not in it yet, keeping the order they come in.
	 * A track without a permalink cannot be queued: there would be nothing to
	 * tell it apart from another, and nothing to mark when its turn comes.
	 *
	 * @returns how many were added.
	 */
	async add(tracks: Track[]) {
		const fresh = this.#taken(tracks, (permalink) => this.has(permalink));

		if (fresh.length === 0) return 0;
		await this.#write([...this.#items, ...fresh]);

		return fresh.length;
	}

	/**
	 * Makes the queue exactly these tracks, for an import asked to replace what
	 * is here rather than add to it (state-transfer.js). Whatever was queued is
	 * gone; a track the file names twice still lands once, since the permalink
	 * is the identity either way.
	 *
	 * @returns how many are in it afterwards.
	 */
	async replace(tracks: Track[]) {
		const fresh = this.#taken(tracks, () => false);
		await this.#write(fresh);

		return fresh.length;
	}

	// The entries to take from a list of tracks: those with a permalink, not
	// already held (`held` says what that means for the caller), and each one
	// only once however often the list names it.
	#taken(tracks: Track[], held: (permalink: string) => boolean) {
		const fresh: QueueEntry[] = [];

		for (const track of tracks) {
			if (!track.permalink) continue;

			if (held(track.permalink)) continue;

			if (fresh.some((entry) => entry.permalink === track.permalink)) continue;
			fresh.push(entryOf(track));
		}

		return fresh;
	}

	remove(permalink: string) {
		return this.removeMany([permalink]);
	}

	/** Takes a whole set of tracks out of the queue in one write. */
	removeMany(permalinks: Iterable<string>) {
		const gone = new Set(permalinks);

		return this.#write(this.#items.filter((item) => !gone.has(item.permalink)));
	}

	toggle(track: Track) {
		return this.has(track.permalink) ? this.remove(track.permalink) : this.add([track]);
	}

	clear() {
		return this.#write([]);
	}

	// The queue in memory, and the mirror beside it — which is nothing but a
	// copy of it, so every assignment to one is an assignment to the other.
	#hold(items: QueueEntry[]) {
		this.#items = items;
		remember(items);
	}

	// The queue on screen is the queue in memory: it is written back on every
	// change, but a write that cannot happen (no session storage) only costs the
	// queue its life beyond this popup.
	//
	// `$state.snapshot` is not a nicety here: what `$state` holds are proxies,
	// and the structured clone every storage write goes through refuses one.
	async #write(items: QueueEntry[]) {
		this.#hold(items);
		this.#written = true;

		try {
			// The one thing that does have to wait for the door: a write made
			// before the access level is set would land in an area the content
			// script cannot read, and the page would paint no queued pills.
			await this.#opened;
			await chrome.storage.session.set({ [KEY]: $state.snapshot(items) });
		} catch (_e) {
			// Nothing to do: the panel already shows what it holds.
		}
	}
}

export const queue = new Queue();
