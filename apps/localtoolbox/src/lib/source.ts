// Where a panel's tracks come from: the page in the active tab, or the
// session queue (see queue.svelte.ts).
//
// Two of the tools can work from either, and the choice is one remembered
// value rather than one per tool: picking a batch out on a page and then
// working through it is a mode. The queue sheet switches the tool it sends a
// batch to into it, and the tool stays there until the page is chosen again.
//
// One object, made here, because the sheet can be over the very tool it sends
// to: two `persisted()` objects over one key would be two copies of the value
// (the same reasoning as downloader.svelte.ts).

import { choicePref } from './prefs.svelte.ts';

export const PAGE = 'page';

export const QUEUE = 'queue';

export const SOURCE_PREF = 'pref:source';

const SOURCES = [PAGE, QUEUE];

export const source = choicePref(SOURCE_PREF, PAGE, SOURCES);
