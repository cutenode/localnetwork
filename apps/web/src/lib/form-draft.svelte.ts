import { tick, untrack } from 'svelte';

/**
 * Unsent forms kept in this browser, so a reload or a trip to another page doesn't lose what was typed.
 * A draft is a convenience of this browser's, like the hotkeys preference: nothing on the server reads it.
 */
type StoredDraft<T> = {
	savedAt: string;
	/** The form's own values when the draft was saved, to tell when they changed since. */
	base: string;
	values: T;
};

const PREFIX = 'ln:draft:';

/** The storage key of `form`'s draft for the signed-in account, so each account on a browser keeps its own. */
export const draftKey = (viewer: { did: string } | null | undefined, form: string) =>
	viewer ? `${PREFIX}${viewer.did}:${form}` : undefined;

function stored<T>(key: string): StoredDraft<T> | null {
	try {
		const raw = localStorage.getItem(key);

		// SAFETY: only `write` stores under the draft prefix, and only drafts of the form whose key it is.
		return raw ? (JSON.parse(raw) as StoredDraft<T>) : null;
	} catch {
		// Blocked storage or a value this code didn't write: no draft.
		return null;
	}
}

function write(key: string, draft: StoredDraft<unknown> | null) {
	try {
		if (draft) localStorage.setItem(key, JSON.stringify(draft));
		else localStorage.removeItem(key);
	} catch {
		// Blocked or full storage: the form still works, it just isn't kept.
	}
}

/** Drops every draft the account `did` kept in this browser, as it signs out. */
export function clearDrafts(did: string) {
	try {
		const prefix = `${PREFIX}${did}:`;

		for (const key of Object.keys(localStorage)) if (key.startsWith(prefix)) localStorage.removeItem(key);
	} catch {
		// Blocked storage holds no drafts.
	}
}

/** Whether a draft is waiting under `key`. */
export const hasDraft = (key: string | undefined) => !!key && stored(key) !== null;

// Every way a form's fields change: typing, picking, and the buttons that add and remove rows or tags.
const EDITS = ['input', 'change', 'click', 'keydown', 'focusout'] as const;

/**
 * Keeps a form's unsent values as a draft under the storage key `key` returns (no key, no draft). Attach `attach` to the form, show
 * `values` in place of the form's own values while it's set, and redraw the form's fields when `revision`
 * changes. `read` turns the form into the values it shows; the draft goes away once they match the form's
 * own values again, on `discard`, or on `sent`.
 */
export class FormDraft<T> {
	/** The restored draft's values, or null while the form shows its own. */
	values = $state<T | null>(null);

	/** When the restored draft was last saved. */
	savedAt = $state('');

	/** Whether the form's own values changed after the draft was saved, like a moderator publishing an edit. */
	stale = $state(false);

	/** Bumps whenever the form has to redraw its fields from other values. */
	revision = $state(0);

	#key: () => string | undefined;

	#read: (form: HTMLFormElement) => T;

	#redraw: () => void;

	#base = '';

	/** `redraw` resets whatever the form keeps of its own values, like its rows, before `revision` bumps. */
	constructor(key: () => string | undefined, read: (form: HTMLFormElement) => T, redraw: () => void = () => {}) {
		this.#key = key;
		this.#read = read;
		this.#redraw = redraw;
	}

	attach = (form: HTMLFormElement) => {
		const key = this.#key();

		if (!key) return;
		let live = true;
		let queued = false;

		const save = () => {
			if (queued) return;
			queued = true;
			// After the edit's own update has drawn, so added and removed rows are in the form.
			void (async () => {
				await tick();
				queued = false;

				if (live) this.#save(key, form);
			})();
		};

		// Once the fields' own enhancements have drawn, so the base reads like every later save.
		void (async () => {
			await tick();

			if (!live) return;
			untrack(() => this.#restore(key, form));

			for (const type of EDITS) form.addEventListener(type, save);
		})();

		return () => {
			live = false;

			for (const type of EDITS) form.removeEventListener(type, save);
		};
	};

	#restore(key: string, form: HTMLFormElement) {
		this.#base = JSON.stringify(this.#read(form));
		const draft = stored<T>(key);

		if (!draft) return;
		this.values = draft.values;
		this.savedAt = draft.savedAt;
		this.stale = draft.base !== this.#base;
		this.#redraw();
		this.revision++;
	}

	#save(key: string, form: HTMLFormElement) {
		const values = this.#read(form);

		write(
			key,
			JSON.stringify(values) === this.#base ? null : { savedAt: new Date().toISOString(), base: this.#base, values }
		);
	}

	/** Drops the draft and redraws the form from its own values. */
	discard = () => {
		const key = this.#key();

		if (key) write(key, null);
		this.values = null;
		this.#redraw();
		this.revision++;
	};

	/** Drops the draft once the form went through, keeping what it shows as the form's own values. */
	sent = (form: HTMLFormElement) => {
		const key = this.#key();

		if (!key) return;
		write(key, null);
		this.#base = JSON.stringify(this.#read(form));
		this.values = null;
	};
}
