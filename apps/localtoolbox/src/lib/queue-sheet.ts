// Fetching the queue sheet's chunk.
//
// The sheet is not a tool — it comes up over whichever one is open, from a
// button in the header of every view — so it is not in the registry and has no
// route. What it shares with the panels is the reason for being its own
// chunk: it brings the component library with it, and the tool menu the popup
// boots into is deliberately free of that (see $lib/tools.ts, and the note in
// ThemeToggle.svelte about why the menu's own controls are plain HTML).
//
// Fetched at most once however often the sheet is opened and closed.

import type QueueSheet from './components/QueueSheet.svelte';

export type Sheet = typeof QueueSheet;

/** Where the chunk comes from: the one place a test can make it fail to arrive, as `tool.load` is for a panel. */
export const sheetChunk = { load: () => import('./components/QueueSheet.svelte') };

let chunk: Promise<Sheet> | null = null;

/**
 * The sheet component, loading its chunk if this is the first ask.
 *
 * A chunk that failed to arrive is not remembered as a failure, for the reason
 * `loadPanel` gives: the network is not involved, but a retry costs nothing
 * and a queue that can never be opened again would cost the session's work.
 */
export function loadQueueSheet() {
	chunk ??= sheetChunk.load().then(
		(module) => module.default,
		(error) => {
			chunk = null;
			throw error;
		}
	);

	return chunk;
}
