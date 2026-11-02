// Fetching the settings sheet's chunk.
//
// The same bargain the queue sheet strikes (queue-sheet.ts): it is not a tool,
// it comes up over whichever one is open, and it brings the component library
// with it — which the tool menu the popup boots into is deliberately free of.
// It is also the one place the state file is read or written, so the script
// that does that (state-transfer.js) comes with it rather than with the page.
//
// Fetched at most once however often the sheet is opened and closed.

import type SettingsSheet from './components/SettingsSheet.svelte';
import { loadTransfer } from './globals.ts';

export type Sheet = typeof SettingsSheet;

/** Where the chunk comes from: the one place a test can make it fail to arrive, as `tool.load` is for a panel. */
export const sheetChunk = {
	load: () => Promise.all([import('./components/SettingsSheet.svelte'), loadTransfer()]).then(([module]) => module)
};

let chunk: Promise<Sheet> | null = null;

/**
 * The sheet component, loading its chunk if this is the first ask.
 *
 * A chunk that failed to arrive is not remembered as a failure, for the reason
 * `loadPanel` gives: the network is not involved, but a retry costs nothing
 * and settings that could never be opened again would cost the extension its
 * one way of turning anything off.
 */
export function loadSettingsSheet() {
	chunk ??= sheetChunk.load().then(
		(module) => module.default,
		(error) => {
			chunk = null;
			throw error;
		}
	);

	return chunk;
}
