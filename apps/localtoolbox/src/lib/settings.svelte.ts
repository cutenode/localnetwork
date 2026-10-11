// The settings that belong to the extension rather than to any one tool.
//
// A panel's options are the panel's own and live in the popup's localStorage
// (prefs.svelte.ts): they say how that panel draws a list, and nothing outside
// it ever asks. These are the other kind — settings that change what the
// extension does somewhere the popup is not, which at present means what the
// content script does to a SoundCloud page.
//
// So they cannot live where the prefs do. The popup's localStorage belongs to
// the popup page; a content script cannot see it, and a setting it cannot see
// is a setting it cannot obey. They live in chrome.storage.local instead,
// beside the marks and the details cache — one object under one key, read by
// both sides and written only here.
//
// The defaults are duplicated in content.ts, which cannot import this file
// (it is a classic script in the page, with no imports at all). One key, one
// shape, one default per setting, held in step by test/consistency.test.ts.

import type { PageSettings } from '$lib/types';
import { fallback, z } from './zod.ts';

const KEY = 'settings';

/**
 * Every setting, and what it is before anyone has touched it.
 *
 * `replaceDownloadButtons` is on: somebody who installs this extension for
 * its download tools is not looking for SoundCloud's own buttons, and a
 * feature that does nothing until a switch is found is a feature nobody
 * finds. Turning it off puts the page back exactly as SoundCloud drew it.
 *
 * `likesListView` is off, for the opposite reason: it presses a button on
 * SoundCloud's own page and SoundCloud remembers the press, so it outlives
 * the visit it was made in. A setting that changes how somebody's Likes page
 * opens from now on is one to be asked for rather than arrived at.
 */
export const DEFAULTS: PageSettings = {
	replaceDownloadButtons: true,
	likesListView: false
};

/** Stored values as settings: anything of the wrong type falls back, and so does no object at all. */
const Stored = fallback(
	z.object({
		replaceDownloadButtons: fallback(z.boolean(), DEFAULTS.replaceDownloadButtons),
		likesListView: fallback(z.boolean(), DEFAULTS.likesListView)
	}),
	() => ({ ...DEFAULTS })
);

class Settings {
	/** What is stored, or the default for anything not. */
	#values: PageSettings = $state({ ...DEFAULTS });

	#started = false;

	get replaceDownloadButtons() {
		return this.#values.replaceDownloadButtons;
	}

	set replaceDownloadButtons(on) {
		void this.#set('replaceDownloadButtons', !!on);
	}

	get likesListView() {
		return this.#values.likesListView;
	}

	set likesListView(on) {
		void this.#set('likesListView', !!on);
	}

	/** Reads the settings and keeps following them. Safe to call more than once. */
	async start() {
		if (this.#started) return;
		this.#started = true;

		try {
			this.#values = Stored.parse((await chrome.storage.local.get(KEY))[KEY]);
		} catch (_e) {
			// No storage to read: the defaults stand for as long as the popup is
			// open, and nothing is written back over whatever is really stored.
			return;
		}

		chrome.storage.onChanged.addListener((changes, area) => {
			// Another window's settings sheet, or an import that replaced them.
			if (area === 'local' && changes[KEY]) this.#values = Stored.parse(changes[KEY].newValue);
		});
	}

	// The switch on screen already shows the new value, so a write that could
	// not happen costs this opening of the popup and nothing more. The whole
	// object goes back every time: it is a handful of flags, and reading first
	// would let two windows racing each other lose one.
	//
	// `$state.snapshot` is not a nicety: what `$state` holds are proxies, and
	// the structured clone every storage write goes through refuses one.
	async #set(name: keyof PageSettings, value: boolean) {
		this.#values = { ...this.#values, [name]: value };

		try {
			await chrome.storage.local.set({ [KEY]: $state.snapshot(this.#values) });
		} catch (_e) {
			// Nothing to do: the switch already shows what it holds.
		}
	}
}

export const settings = new Settings();
