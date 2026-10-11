// Where a panel's tracks come from: the page in the active tab, or the
// session queue (see queue.svelte.ts).
//
// Two of the three tools can work from either, and the choice is one
// remembered value rather than one per tool: picking a batch out on a page and
// then working through it is a mode — the queue sheet switches the tool it
// sends a batch to into it, and the tool stays there until the page is chosen
// again.

export const PAGE = 'page';

export const QUEUE = 'queue';

export const SOURCE_PREF = 'pref:source';

export const SOURCES = [PAGE, QUEUE];
