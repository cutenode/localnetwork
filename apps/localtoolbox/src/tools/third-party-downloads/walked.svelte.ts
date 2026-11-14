// The Downloader's list, kept for as long as the browsing session lasts.
//
// Why it exists: this panel hands work back to a person. Most of these gates
// fail for something a fetch can never give them (a connected account, a
// CAPTCHA, a step the page does to itself), and the way a row hands that over
// is a tab. Opening a tab closes the popup.
//
// Without this, everything the walk has learned would go with it: which links
// have been saved, which have resolved to a page to open rather than a file,
// which have failed and why, and the links pasted into the drawer, which no
// source would ever offer again. The list would also be built afresh from
// whatever tab is in front, and the tab in front is usually the gate that has
// just been opened, which is a link this tool claims: twenty rows would come
// back as one.
//
// So the list itself is kept, beside the session queue and the popup's place
// (see $lib/queue.svelte.ts and $lib/view.svelte.ts), in
// chrome.storage.session. A working list, not a library: it goes when the
// browser does, and the marks it makes are what lasts.
//
// Two things are held. `listed` is the list on screen, in its order. `known` is
// every row this session has built, by URL, outcomes and all: a link
// deliberately switched away from (the page for the queue, say) is not on
// screen any more, but walking it again from the other source would be walking
// it twice, so what became of it is remembered whether it is listed or not.
//
// A gate done on its own page is finished while the popup is closed, so what
// became of it is the background worker's to say (./on-page.ts). It keeps that
// under a key of its own rather than writing this list, which an open panel
// would then write over; the list reads it on start and follows it after.
//
// What is deliberately not kept:
//
//   the site      a module with functions on it, which no storage takes; a row
//                 finds its site again from its own URL, and a row whose site
//                 this version no longer knows is dropped rather than shown
//                 with nothing to walk it.
//   a walk under  a run dies with the popup that started it, so a row caught
//   way           mid-walk comes back as it was before Get was pressed. What
//                 happened to the one file that may have been handed to the
//                 browser in that moment is the downloads list's to say.

import type { GateOutcome, Track } from '$lib/types';
import { storedTrack } from '$lib/stored';
import { fallback, z } from '$lib/zod';
import { siteFor, type Site } from './sites/index.ts';
import { EMAIL, type NextStep } from './next-step.ts';
import { OUTCOMES, Outcomes } from './on-page.ts';

/**
 * Where a row's walk has got to. Not run, under way (`working`, then `saving` once the file is
 * known), opened on its own page to be done there (`opened`), or come to something: saved
 * (`done`), signed up for (`sent`), or `failed`.
 */
const RowState = z.enum(['queued', 'working', 'saving', 'opened', 'done', 'sent', 'failed']);

export type RowState = z.infer<typeof RowState>;

/** One link on the list, and what became of it (Panel.svelte addRows builds them). */
export interface Row {
	/** The gate link. */
	url: string;
	site: Site;
	/** Its track's path, or '' for a link with no track behind it. */
	permalink: string;
	/** The whole track, which opening a failed gate queues. */
	track: Track | null;
	label: string;
	state: RowState;
	/** The line under the label: a failure's reason, a note, or ''. */
	detail: string;
	/** Where Open leads once it has run, or ''. */
	openUrl: string;
	/** What a failed row offers to do about it. */
	next: '' | 'open' | NextStep;
	/** Whether it was pasted into the drawer. */
	byHand: boolean;
}

/** A row as it is kept (see above). */
type RowRecord = Omit<Row, 'site'>;

// A kept row read back. Only this module writes one, but a field holding what
// a row cannot reads as the row's own default for it.
const RowRecord = z.object({
	url: z.string(),
	permalink: fallback(z.string(), ''),
	track: fallback(z.nullable(storedTrack<Track>()), null),
	label: fallback(z.string(), ''),
	state: fallback(RowState, 'queued'),
	detail: fallback(z.string(), ''),
	openUrl: fallback(z.string(), ''),
	next: fallback(z.enum(['', 'open', EMAIL]), ''),
	byHand: fallback(z.boolean(), false)
}) satisfies z.ZodMiniType<RowRecord>;

/** What is kept under KEY. */
const Kept = z.object({
	origin: fallback(z.string(), ''),
	/** The URLs on screen, in order. */
	listed: fallback(z.array(z.string()), []),
	/** Every row this session has built, by URL. */
	known: fallback(z.record(z.string(), RowRecord), {})
});

export type Kept = z.infer<typeof Kept>;

const KEY = 'downloader';

// A walk that is still going when the popup closes will not finish: nothing was
// saved and nothing failed, so the row that is kept is the one it was before
// Get was pressed. It is kept that way rather than put back that way, so no
// reader of the stored list has to know what a running row means.
const RESTING: Partial<Record<RowState, RowState>> = { working: 'queued', saving: 'queued' };

/** The fields of a row that outlive the popup. */
const recordOf = (row: Row): RowRecord => ({
	url: row.url,
	permalink: row.permalink,
	track: row.track,
	label: row.label,
	state: RESTING[row.state] ?? row.state,
	detail: row.detail,
	openUrl: row.openUrl,
	next: row.next,
	byHand: row.byHand
});

class Walked {
	/** The list on screen, in order, each row with its site. */
	#rows: Row[] = $state([]);

	// Not reactive on purpose: it is a lookup table the markup never reads, so
	// making it so would only cost a proxy on every write.
	#known = new Map<string, RowRecord>();

	/** Where the listed links were taken from, for the line under the title. */
	#origin = $state('');

	#started = false;

	#writing = false;

	get rows() {
		return this.#rows;
	}

	get origin() {
		return this.#origin;
	}

	set origin(value) {
		this.#origin = value;
		this.#write();
	}

	/** The links pasted in by hand, which no source offers again. */
	get byHand() {
		return [...this.#known.values()].flatMap((record) => (record.byHand ? [record.url] : []));
	}

	/** Reads the list back. Safe to call more than once. */
	async start() {
		if (this.#started) return;
		this.#started = true;
		let kept;

		try {
			kept = (await chrome.storage.session.get(KEY))[KEY];
		} catch (_e) {
			// No session storage to read: the list lives as long as the popup does.
			return;
		}

		const stored = Kept.safeParse(kept);

		if (stored.success) {
			this.#origin = stored.data.origin;

			for (const [url, record] of Object.entries(stored.data.known)) this.#known.set(url, record);
			this.#rows = stored.data.listed.flatMap((url) => this.#restore(url) ?? []);
		}

		// Followed on a session with no list yet too: the first Get of one hands
		// the worker gates whose outcomes this popup is still open to see.
		await this.#follow();
	}

	/** Takes in what the worker has said about gates done on their own page, now and from here on. */
	async #follow() {
		try {
			this.#apply(Outcomes.parse((await chrome.storage.session.get(OUTCOMES))[OUTCOMES]));
			chrome.storage.onChanged.addListener((changes, area) => {
				if (area === 'session' && changes[OUTCOMES]) this.#apply(Outcomes.parse(changes[OUTCOMES].newValue));
			});
		} catch (_e) {
			// Nothing to follow: rows say what this popup saw.
		}
	}

	#apply(outcomes: Record<string, GateOutcome>) {
		let changed = false;

		for (const [url, outcome] of Object.entries(outcomes)) {
			const row = this.listed(url);
			const record = row ?? this.#known.get(url);

			if (!record || (record.state === outcome.state && record.detail === outcome.detail)) continue;
			record.state = outcome.state;
			record.detail = outcome.detail;
			record.next = '';
			record.openUrl = '';

			if (outcome.label) record.label = outcome.label;
			changed = true;
		}

		if (changed) this.#write();
	}

	/** The row on screen for this link, if it is on screen. */
	listed(url: string): Row | null {
		return this.#rows.find((row) => row.url === url) ?? null;
	}

	/**
	 * Puts a link at the end of the list, as the row it already was, if this
	 * session has built one for it. Answers with the row as the list holds it,
	 * which is what a walk then writes its state to.
	 */
	add(row: Row) {
		const before = this.#known.get(row.url);
		const merged = before ? { ...row, ...before, site: row.site } : row;

		// A link pasted in by hand has no track behind it; the page it turns out
		// to be on does, and a row that has not been walked yet is better named by
		// the track than by the tail of its URL.
		if (before && !merged.permalink && row.permalink) {
			merged.permalink = row.permalink;
			merged.track = row.track;

			if (merged.state === 'queued' && row.label) merged.label = row.label;
		}

		this.#rows.push(merged);
		this.#write();

		return this.#rows[this.#rows.length - 1];
	}

	/** Writes the list back after a walk has changed a row under it. */
	save() {
		this.#write();
	}

	/**
	 * Empties the list, for a source deliberately switched: what is listed is
	 * what the chosen source holds. What became of each row is not forgotten:
	 * a link the other source offers too is still the row it was.
	 */
	clear() {
		this.#fold();
		this.#rows = [];
		this.#origin = '';
		this.#write();
	}

	/** Takes down what the listed rows hold now, listed or not from here on. */
	#fold() {
		for (const row of this.#rows) this.#known.set(row.url, recordOf(row));
	}

	/** The stored record for `url` as a row again, or null for a lost site. */
	#restore(url: string): Row | null {
		const record = this.#known.get(url);
		const site = record ? siteFor(url) : null;

		return site ? { ...record!, site } : null;
	}

	// One write per turn, however many rows moved in it: a run settles a row and
	// renames it in the same breath, and that is one list, not two.
	//
	// `$state.snapshot` is required: what `$state` holds are proxies, and
	// the structured clone every storage write goes through refuses one.
	#write() {
		if (this.#writing) return;
		this.#writing = true;
		// Nothing to wait for: #flush catches the one thing in it that can fail.
		queueMicrotask(() => void this.#flush());
	}

	async #flush() {
		this.#writing = false;
		this.#fold();

		const stored = {
			origin: this.#origin,
			listed: this.#rows.map((row) => row.url),
			known: Object.fromEntries(this.#known)
		};

		try {
			await chrome.storage.session.set({ [KEY]: $state.snapshot(stored) });
		} catch (_e) {
			// Nothing to do: the panel already shows what it holds.
		}
	}
}

export const walked = new Walked();
