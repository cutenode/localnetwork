// What the Third-Party Downloader hands to a gate that asks, and what it does
// once a file has landed.
//
// These are options in the sense prefs.svelte.ts means it: kept for good, in
// the popup page's own localStorage, under the keys they have always had. They
// are *set* in the settings sheet rather than in a drawer on the panel. A
// panel's options say how the list in front of you behaves and are read a foot
// from where they are set; an address and a name are neither. They are typed
// once, on the day one gate asks for them, and never thought about again. The
// mark is of the same kind: it says what saving a file should do, not what the
// list should show.
//
// So they are made here, once, rather than by each component that shows them.
// The sheet is where the address is typed and the panel is where it is used, a
// moment later and in another component entirely; two `persisted()` objects
// over one key would be two copies of the value, and the panel would still be
// holding the empty one it read as it mounted. One object, imported by both,
// is what makes what was typed in the sheet the answer the panel gives a gate.
//
// Not in chrome.storage, where settings.svelte.ts's live: those are there
// because the content script has to read them, and nothing outside the
// extension's own pages has any use for these. The one other page that reads
// them is the offscreen page the background worker opens (src/offscreen.ts),
// which shares the popup's origin and so its localStorage: it walks a gate for
// the page's Download button, and hands the worker the address gate-page.ts
// fills in on a gate's own page.
// Nor are they in the exported state file: the keys it carries are listed in
// state-transfer.ts, and an address is not something to ship to another
// install with somebody's marks.

import { flagPref, textPref } from './prefs.svelte.ts';

export const downloader = {
	/** The address a Hypeddit gate that asks is given, and a Droploud or Laylo page is filled in with. */
	email: textPref('pref:email'),

	/** The name a Hypeddit gate's email step sends beside it. */
	name: textPref('pref:name'),

	/**
	 * Whether saving a file marks its SoundCloud track as downloaded.
	 *
	 * On: a walk that ends in a file is the surest moment there is that a track
	 * really was had, and the mark is what the rest of the extension goes by.
	 */
	mark: flagPref('pref:mark', true)
};
