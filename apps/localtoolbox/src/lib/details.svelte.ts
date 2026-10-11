// How many tracks the extension has looked up, as something a panel can show.
//
// The lookups themselves — what SoundCloud said about a track, and the
// failures worth not asking about again — are the content script's
// (content.ts, DETAILS_KEY). This reads nothing but their number, which is the
// one thing the popup has any use for: the export section says what is about
// to go into the file, and a count is what makes "export" mean something
// before it has been pressed.
//
// The same shape as downloaded.svelte.ts, and for the same reason: one read,
// then every storage change followed, so a lookup made in a SoundCloud tab or
// an import run in another window moves the number here too.

import { transfer } from './globals.ts';
import { fallback, z } from './zod.ts';

/** Entries in a stored cache, counting nothing for anything that is not one. */
const Size = z.pipe(
	fallback(z.record(z.string(), z.unknown()), {}),
	z.transform((cache) => Object.keys(cache).length)
);

class Details {
	#count = $state(0);

	#started = false;

	get count() {
		return this.#count;
	}

	/** Reads the cache's size and keeps following it. Safe to call more than once. */
	async start() {
		if (this.#started) return;
		this.#started = true;
		const key = transfer().DETAILS_KEY;

		try {
			this.#count = Size.parse((await chrome.storage.local.get(key))[key]);
		} catch (_e) {
			// No storage to read: nothing has been looked up as far as anyone here
			// is concerned, and the export still writes whatever really is there.
			return;
		}

		chrome.storage.onChanged.addListener((changes, area) => {
			// A SoundCloud tab that looked something up, another window's import, or
			// this one's.
			if (area === 'local' && changes[key]) this.#count = Size.parse(changes[key].newValue);
		});
	}
}

export const details = new Details();
