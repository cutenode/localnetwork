// Which of the search badges are switched on.
//
// The sites themselves are data (./search-sites.json, read by
// ./search-sites.ts); this is the other half — one switch per site, thrown in
// the settings sheet and read by the Manager panel a moment later, in another
// component entirely.
//
// So they are made here, once, for the reason the Downloader's options are
// (downloader.svelte.ts): two `flagPref()` objects over one key would be two
// copies of the value, and the panel would still be drawing badges from the
// one it read as it mounted.
//
// In the popup's own localStorage, not in chrome.storage: nothing outside the
// popup draws these badges, so nothing outside it has any use for the answer.
// Nor are they in the exported state file — the keys it carries are listed in
// state-transfer.ts, and which four shops somebody searches is not worth a
// line in a format every install has to keep reading.
//
// One key per site, built from the site's own id, so a site added to the JSON
// file brings its switch with it and needs no line here.

import { flagPref } from './prefs.svelte.ts';
import { SEARCH_SITES } from './search-sites.ts';

/** Where one site's switch is remembered. Its id is part of the key. */
export const searchPrefKey = (id: string) => `pref:search:${id}`;

// Made at import, one per site, and never added to: the file is read once and
// the popup lives for minutes.
const switches = new Map(SEARCH_SITES.map((site) => [site.id, flagPref(searchPrefKey(site.id), site.on)]));

export const searchSites = {
	/** Every site there is, each with whether its badge is shown. */
	get all() {
		return SEARCH_SITES.map((site) => ({ ...site, on: switches.get(site.id)!.value }));
	},

	/** Only the ones switched on, in the file's order — what a row draws. */
	get enabled() {
		return SEARCH_SITES.filter((site) => switches.get(site.id)!.value);
	},

	/**
	 * Throws one switch. The panel under the sheet redraws from the same object.
	 *
	 * @param id one of SEARCH_SITES
	 */
	set(id: string, on: boolean) {
		switches.get(id)!.value = on;
	}
};
