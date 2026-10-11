// Where the popup was when it last closed: which tool was open, and the parts
// of each panel that are the user's doing rather than the page's — a search
// typed into a filter bar, a drawer pulled open, a long list scrolled down.
//
// Why it exists: a browser-action popup is destroyed on every click that takes
// the focus off it, which in these tools is most clicks — opening a download
// link, going to a gate tab, saving a file. Re-opening it landed back on the
// tool menu with every panel blank, so working through a page meant finding
// the place in it again each time. The options were already remembered
// (prefs.svelte.ts); this is the rest of what was on screen.
//
// Where it lives: chrome.storage.session, beside the session queue and for the
// same reason — this is where someone is in a piece of work, not a setting. It
// goes when the browser does, and a new session opens on the tool menu.
//
// What it deliberately does not keep: anything a page or a run owns. The
// tracks read from a tab are read again (by then the tab may be another page
// altogether), a half-finished run of the Downloader is not resumed, and a
// confirm button that was armed comes back disarmed. Those are answers, not
// places.
//
// Reading it is asynchronous, and the popup used to draw nothing at all until
// it came back — restoring after the first paint would have shown the tool
// menu for a frame and then replaced it. Drawing nothing is the worse of the
// two: the popup is blank for as long as the browser takes to answer, every
// single time it is opened, to spare a flash that happens once.
//
// So which tool was open is mirrored into the popup's own localStorage, which
// is *synchronous*: `open` answers with that mirror before the session has
// been read, and the shell has a route on the first line it runs — a colour
// to paint, a header to draw, a chunk to fetch and a page read to start (see
// $lib/tools.ts). What the mirror cannot know is when a browsing session
// ended, since localStorage outlives one; session storage is still what
// settles that, and `ready` still says when it has. Until then the shell
// draws the panel's frame around a ghost of its body rather than the panel
// itself — panels read their own remembered values synchronously as they
// mount (`remembered()`), so they must not mount before those have arrived.

import { fallback, z } from './zod.ts';

const KEY = 'view';

/** What a panel keeps: the kinds of value its `remembered()` places are. */
const Kept = z.union([z.string(), z.number(), z.boolean(), z.null(), z.array(z.string())]);

type Kept = z.output<typeof Kept>;

/** What `#write` keeps under KEY, as far as it can be read back; anything else is nothing kept. */
const StoredView = fallback(
	z.nullable(
		z.object({
			open: fallback(z.nullable(z.string()), null),
			queue: fallback(z.boolean(), false),
			settings: fallback(z.boolean(), false),
			panels: fallback(z.record(z.string(), z.record(z.string(), Kept)), {})
		})
	),
	null
);

/** What `#write` keeps under KEY. */
export type StoredView = NonNullable<z.output<typeof StoredView>>;

// Where the mirror lives. Not a `pref:` key: it is not a setting, it is not
// in the exported state file (state-transfer.js lists what is), and it is
// rewritten on every navigation.
const HINT = 'view:open';

const hint = () => {
	try {
		return localStorage.getItem(HINT) || null;
	} catch (_e) {
		// No storage to read: the popup draws the menu until the session answers.
		return null;
	}
};

const rememberHint = (id: string | null) => {
	try {
		if (id) localStorage.setItem(HINT, id);
		else localStorage.removeItem(HINT);
	} catch (_e) {
		// Ignore: the next opening starts from the session read instead, which is
		// exactly where it started before there was a mirror.
	}
};

class View {
	/** The open tool's id, or null for the menu. */
	#open: string | null = $state(null);

	/** What localStorage says was open, read before anything else runs. */
	#hint: string | null = hint();

	/** Scope -> what it kept. */
	#panels: StoredView['panels'] = $state({});

	/** Whether the session queue's sheet is up over whatever is open. */
	#queue = $state(false);

	/** Whether the settings sheet is up over whatever is open. */
	#settings = $state(false);

	#ready = $state(false);

	#started = false;

	#writing = false;

	/** Whether the stored place has been read; nothing should draw before it. */
	get ready() {
		return this.#ready;
	}

	/**
	 * The open tool's id, or null for the menu.
	 *
	 * Before the session has been read this is the localStorage mirror — a
	 * good guess, and the only one available synchronously. After it, it is
	 * what the session actually holds, which is what a new browsing session
	 * corrects to the menu.
	 */
	get open() {
		return this.#ready ? this.#open : this.#hint;
	}

	set open(id) {
		this.#open = id ?? null;
		rememberHint(this.#open);
		this.#write();
	}

	/**
	 * Whether the queue sheet is up.
	 *
	 * A place like any other here: the sheet is reached from the header of
	 * every view rather than by opening a tool, and the popup closing behind a
	 * click is no reason to put it away. Shut until the session has been read,
	 * for the same reason the panels wait — nothing should be drawn over a
	 * view that is still a ghost.
	 */
	get queueOpen() {
		return this.#ready && this.#queue;
	}

	set queueOpen(open) {
		this.#queue = !!open;

		// One sheet at a time: both come up over the whole popup from the same
		// row of buttons, and a settings sheet under a queue sheet is a settings
		// sheet nobody can see or get out of.
		if (this.#queue) this.#settings = false;
		this.#write();
	}

	/**
	 * Whether the settings sheet is up.
	 *
	 * A place like the queue's, kept for the same reasons: it is reached from
	 * the header of every view, and the popup closing behind a click is no
	 * reason to put it away. Shut until the session has been read.
	 */
	get settingsOpen() {
		return this.#ready && this.#settings;
	}

	set settingsOpen(open) {
		this.#settings = !!open;

		if (this.#settings) this.#queue = false;
		this.#write();
	}

	/** Reads the place the popup was left in. Safe to call more than once. */
	async start() {
		if (this.#started) return;
		this.#started = true;

		try {
			const stored = StoredView.parse((await chrome.storage.session.get(KEY))[KEY]);

			if (stored) {
				this.#open = stored.open;
				this.#panels = stored.panels;
				this.#queue = stored.queue;
				this.#settings = stored.settings;
			}
		} catch (_e) {
			// No session storage to read: the popup opens on the menu, and holds
			// its place only for as long as it stays open.
		}

		this.#ready = true;
		// The mirror is only ever a guess about the session, and this is where
		// the session says. A new browsing session opens on the menu, so the
		// mirror that pointed at last night's tool is wiped rather than left to
		// paint the wrong header on the next opening too.
		rememberHint(this.#open);
	}

	/**
	 * What `scope` last had under `key`, or `fallback` if it had nothing.
	 *
	 * @returns trusted to be the kind of thing `set` was given under that key
	 */
	get<T extends Kept>(scope: string, key: string, initial: T): T {
		const held = this.#panels[scope]?.[key];

		// SAFETY: `set` is the only writer of a key, and each key is one remembered() place, which
		// only ever hands it the T it was made with.
		return held === undefined ? initial : (held as T);
	}

	set(scope: string, key: string, value: Kept) {
		this.#panels[scope] = { ...this.#panels[scope], [key]: value };
		this.#write();
	}

	// One write per turn, however many values moved in it: a keystroke that
	// changes a search and the count under it is one place, not two. A write
	// that cannot happen costs the place nothing until the popup closes, which
	// is the only time it is ever read back.
	//
	// `$state.snapshot` is not a nicety: what `$state` holds are proxies, and
	// the structured clone every storage write goes through refuses one.
	#write() {
		if (this.#writing) return;
		this.#writing = true;
		queueMicrotask(() => void this.#save());
	}

	async #save() {
		this.#writing = false;

		try {
			await chrome.storage.session.set({
				[KEY]: {
					open: this.#open,
					queue: this.#queue,
					settings: this.#settings,
					panels: $state.snapshot(this.#panels)
				}
			});
		} catch (_e) {
			// Nothing to do: the panel already shows what it holds.
		}
	}
}

export const view = new View();

/** A remembered place: `.value` reads it and writes it through, so `bind:` works on it. */
export interface Remembered<T> {
	value: T;
}

/**
 * A value a panel keeps for the rest of the session, under one tool's name.
 *
 * The same shape as `persisted()` in prefs.svelte.js — `.value` reads and
 * writes, and `bind:` works on it — so a panel does not have to care which of
 * the two a given piece of state is. The difference is what they are for:
 * a pref is a setting, kept for good; this is a place, kept until the browser
 * closes.
 *
 * @param scope the tool's id
 * @param key names the value within the tool
 * @param initial what it is before it has ever been set
 */
export function remembered(scope: string, key: string, initial: string): Remembered<string>;
export function remembered(scope: string, key: string, initial: number): Remembered<number>;
export function remembered(scope: string, key: string, initial: boolean): Remembered<boolean>;
export function remembered<T extends Kept>(scope: string, key: string, initial: T): Remembered<T>;
export function remembered<T extends Kept>(scope: string, key: string, initial: T): Remembered<T> {
	let value = $state(view.get(scope, key, initial));

	return {
		get value() {
			return value;
		},
		set value(next) {
			value = next;
			view.set(scope, key, next);
		}
	};
}
