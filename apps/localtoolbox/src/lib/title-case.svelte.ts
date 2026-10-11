// Whether an opened row's words are shown in Title Case.
//
// The switch over ./title-case.ts, which is the transform itself: the same
// arrangement the search badges have (search-sites.ts and
// search-sites.svelte.ts) — the thing, and the option that says whether to use
// it, made once here so the sheet that throws the switch and the panel
// underneath it are holding the one value. Two `flagPref()` calls over the one
// key would be two copies, and the rows under the sheet would go on drawing
// what the panel read as it mounted.
//
// In the popup's own localStorage rather than in chrome.storage, where
// settings.svelte.ts's live: those are there because the content script has to
// obey them, and nothing outside the popup draws these fields. Nor is it in
// the exported state file — the keys that travel are listed in
// state-transfer.ts, and how somebody likes their own tracklist capitalised is
// not worth a line in a format every install has to keep reading.
//
// Off by default. What the uploader typed is what SoundCloud shows, and an
// extension that quietly retypes it is one whose fields do not match the page
// they came off until somebody asks for that.

import { flagPref } from './prefs.svelte.ts';

/** Whether title, artist and genre are title-cased in an opened row. */
export const titleCasing = flagPref('pref:title-case', false);
