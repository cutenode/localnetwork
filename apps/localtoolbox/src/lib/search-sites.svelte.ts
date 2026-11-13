// Which of the search badges are switched on, and the sites somebody added.
//
// The shipped sites are data (./search-sites.json, read by ./search-sites.ts);
// this is the other half: one switch per site, thrown in the settings sheet
// and read by the Manager panel a moment later, in another component entirely,
// and the sites added in that sheet, in the same shape as the file's.
//
// So they are made here, once, for the reason the Downloader's options are
// (downloader.svelte.ts): two `flagPref()` objects over one key would be two
// copies of the value, and the panel would still be drawing badges from the
// one it read as it mounted.
//
// In the popup's own localStorage, not in chrome.storage: nothing outside the
// popup draws these badges, so nothing outside it has any use for the answer.
// The switches are not in the exported state file either: which shops
// somebody searches is not worth a line in a format every install has to keep
// reading. The added sites are (`searchSites` in state-transfer.ts), since
// typing a site in again on another install is the kind of loss that file is
// there to save.
//
// One key per site, built from the site's own id, so a site added to the JSON
// file or in settings brings its switch with it and needs no line here.

import { SvelteMap } from 'svelte/reactivity';
import type { SearchSiteEntry } from '$lib/types';
import { flagPref, persisted, type Pref } from './prefs.svelte.ts';
import { entryOf, readSites, SEARCH_SITES, type SearchSite } from './search-sites.ts';

/** Where one site's switch is remembered. Its id is part of the key. */
export const searchPrefKey = (id: string) => `pref:search:${id}`;

/** Where the sites added in settings are kept, as a JSON list of entries. */
export const OWN_SITES_KEY = 'pref:search-sites';

/** The sites added in settings. Read the way the file is, so a broken one is dropped. */
const own = persisted<SearchSite[]>(OWN_SITES_KEY, [], {
	parse: (raw) => {
		try {
			return readSites(JSON.parse(raw));
		} catch (_e) {
			// Not JSON: nothing this popup wrote, and nothing to draw.
			return [];
		}
	},
	serialize: (sites) => JSON.stringify(sites.map(entryOf))
});

const sites = () => [...SEARCH_SITES, ...own.value];

// One per site, made as the popup opens for every site there is then, and for
// one added later the first time it is thrown: never inside a getter, since a
// switch made while a row is being drawn is one that row does not follow. A
// SvelteMap, so a row that read a switch not made yet redraws once it is.
// Never dropped: the popup lives for minutes, and a removed site's switch is
// one nothing reads.
const switches = new SvelteMap<string, Pref<boolean>>();

const switchOf = (site: SearchSite) => {
	const made = switches.get(site.id) ?? flagPref(searchPrefKey(site.id), site.on);
	switches.set(site.id, made);

	return made;
};

for (const site of sites()) switchOf(site);

/** Whether a site's badge is shown: its switch, or what it ships as until it has one. */
const isOn = (site: SearchSite) => switches.get(site.id)?.value ?? site.on;

export const searchSites = {
	/** Every site there is, the file's then the added ones, each with whether its badge is shown. */
	get all() {
		return sites().map((site) => ({ ...site, on: isOn(site), own: !SEARCH_SITES.includes(site) }));
	},

	/** Only the ones switched on, in that order: what a row draws. */
	get enabled() {
		return sites().filter(isOn);
	},

	/** The added sites as plain entries, for the state file. */
	get entries(): SearchSiteEntry[] {
		return own.value.map(entryOf);
	},

	/**
	 * Throws one switch. The panel under the sheet redraws from the same object.
	 *
	 * @param id one of `all`
	 */
	set(id: string, on: boolean) {
		switchOf(sites().find((site) => site.id === id)!).value = on;
	},

	/**
	 * Adds sites after the ones there are, from the sheet's form or an imported
	 * file. One whose search is already a badge is left out, so importing the
	 * same file twice adds nothing; one whose id is taken by another search gets
	 * a new id, so two badges never share a switch.
	 *
	 * @returns how many went in
	 */
	add(entries: unknown[]) {
		const added: SearchSite[] = [];

		for (const site of readSites(entries)) {
			const held = [...sites(), ...added];

			if (held.some((one) => one.url === site.url)) continue;
			added.push(held.some((one) => one.id === site.id) ? { ...site, id: crypto.randomUUID() } : site);
		}

		if (added.length > 0) own.value = [...own.value, ...added];

		return added.length;
	},

	/** The added sites become exactly `entries`, for an import asked to replace. */
	replace(entries: unknown[]) {
		own.value = [];

		return searchSites.add(entries);
	},

	/**
	 * Puts an edited site back where it was. One that does not read changes
	 * nothing; the form refuses a search that is another badge's before this.
	 */
	edit(id: string, entry: Omit<SearchSiteEntry, 'id'>) {
		const [site] = readSites([{ ...entry, id }]);

		if (site) own.value = own.value.map((one) => (one.id === id ? site : one));
	},

	remove(id: string) {
		own.value = own.value.filter((one) => one.id !== id);
	}
};
