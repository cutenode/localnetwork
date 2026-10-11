// The state behind the filter bar the Cover Art and Manager panels share:
// a search over whatever text a row calls its own, and a "Hide downloaded"
// toggle that drops the tracks already marked, leaving the ones still to get.
//
// The toggle is an option, remembered for good under the key the exported
// state file carries. The search is a place rather than a setting: it is kept
// for the rest of the browsing session, per tool, so closing the popup to open
// a link and coming back does not clear it — and a new session starts with the
// whole list again (see view.svelte.ts).

import { matches, HIDE_DOWNLOADED_PREF } from './filter.ts';
import { flagPref } from './prefs.svelte.ts';
import { remembered } from './view.svelte.ts';

/** @param scope the id of the tool whose list this filters. */
export function createFilter(scope: string) {
	const query = remembered(scope, 'query', '');
	const hide = flagPref(HIDE_DOWNLOADED_PREF, false);

	return {
		get query() {
			return query.value;
		},
		set query(value) {
			query.value = value;
		},
		get hideDownloaded() {
			return hide.value;
		},
		set hideDownloaded(value) {
			hide.value = value;
		},
		/** Whether a row with this text and mark stays visible. */
		keeps: (text: string, isDownloaded: boolean) => !(hide.value && isDownloaded) && matches(text, query.value)
	};
}
