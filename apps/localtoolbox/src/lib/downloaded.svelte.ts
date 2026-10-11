// The set of tracks marked as downloaded, as something a panel can render.
//
// The marks themselves live in chrome.storage.local and belong to
// downloaded-store.ts — the classic script the content script loads too, so
// that a mark made in a panel is painted on the SoundCloud page and a mark
// made on the page shows up here. This is only a reactive mirror of it: it
// reads the map once, follows every storage change (including ones made in
// another window, or by the page), and hands writes straight back to the
// store.

import type { DownloadedMarks } from '$lib/types';
import { store } from './globals.ts';

class Downloaded {
	/** Permalink path -> ISO time it was marked. */
	#map: DownloadedMarks = $state({});

	#started = false;

	get map() {
		return this.#map;
	}

	get count() {
		return Object.keys(this.#map).length;
	}

	has(permalink: string) {
		return !!this.#map[permalink];
	}

	/** Reads the marks and keeps following them. Safe to call more than once. */
	async start() {
		if (this.#started) return;
		this.#started = true;

		try {
			this.#map = await store().load();
		} catch (_e) {
			// No storage to read (or none granted): nothing is marked.
			this.#map = {};

			return;
		}

		chrome.storage.onChanged.addListener((changes, area) => {
			if (area === 'local' && changes[store().KEY]) {
				// The map the change leaves, by the store's own test; a removed key leaves none.
				const marks = changes[store().KEY].newValue;

				this.#map = store().isMarks(marks) ? marks : {};
			}
		});
	}

	set(permalink: string, value: boolean) {
		return store().set(permalink, value);
	}

	setMany(permalinks: string[], value: boolean) {
		return store().setMany(permalinks, value);
	}

	/**
	 * Flips one mark. Unlike the calls above it never rejects: every caller is
	 * a button's onclick, which has nowhere to put a rejection — and a storage
	 * that refused the write has already left the row saying what it holds.
	 * The calls that do report (`setMany` behind "mark the batch") are awaited
	 * by whoever reports them, and still throw.
	 */
	toggle(permalink: string) {
		return this.set(permalink, !this.has(permalink)).catch(() => {});
	}

	clear() {
		return store().clear();
	}
}

export const downloaded = new Downloaded();
