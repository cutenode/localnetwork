// The tracks ticked in a list, and the mode that shows the tick boxes.
//
// Markers used to be put on a batch of tracks by a modifier-click on one of
// the row buttons: the rows already in the set were read as a selection, and
// the click moved its edge. That asked one button to mean two things, and the
// selection it worked on was the marker itself — so there was no way to look at
// what was about to be marked before marking it, and no way to put *two*
// markers on one batch.
//
// Selecting is its own step now. Turning the mode on puts a tick box on every
// row; ticking rows builds a selection that is nothing but a selection; and the
// bar above the list is where a marker — queued, downloaded — is then put on
// the lot, or taken off again. The row buttons are back to meaning one thing
// each, on one track.
//
// Where it lives: a selection is a *place*, not a setting, so it is kept for
// the browsing session under the tool's own name (view.svelte.ts) rather than
// for good. That matters here more than anywhere — this popup closes on every
// click that opens a tab, and a half-made selection of twenty tracks is not
// something to ask anyone for twice.

import { SvelteSet } from 'svelte/reactivity';
import { remembered } from './view.svelte.ts';

/**
 * A selection over a list of tracks, identified by permalink.
 *
 * @param scope the id of the tool whose list this selects within.
 */
export function createSelection(scope: string) {
	const active = remembered(scope, 'selecting', false);
	// The set is what the panel asks row by row, so a list hundreds long costs
	// one lookup per row; the remembered array beside it is the same thing in the
	// shape session storage will take, written whenever the set changes.
	const stored: { value: string[] } = remembered(scope, 'picked', []);
	const ticked = new SvelteSet(stored.value);

	// The row a Shift-click reaches from: the last one ticked by hand. Not kept
	// for the session — it is half of a gesture, and the rows it would reach
	// between are whatever the list is showing now.
	let anchor = $state<string | null>(null);

	const save = () => {
		stored.value = [...ticked];
	};

	/**
	 * Ticks a whole set of rows, or unticks them.
	 *
	 * @param permalinks an empty one is skipped
	 */
	function pick(permalinks: Iterable<string>, on: boolean) {
		for (const permalink of permalinks) {
			if (!permalink) continue;

			if (on) ticked.add(permalink);
			else ticked.delete(permalink);
		}

		save();
	}

	function empty() {
		ticked.clear();
		anchor = null;
		save();
	}

	return {
		/** Whether the tick boxes are showing. Turning them off unticks the lot. */
		get active() {
			return active.value;
		},
		set active(on) {
			active.value = on;

			if (!on) empty();
		},

		/** The ticked permalinks. The caller's list is what gives them an order. */
		get permalinks() {
			return [...ticked];
		},

		get count() {
			return ticked.size;
		},

		/** The row a Shift-click would reach from, or null if there is none. */
		get anchor() {
			return anchor;
		},

		has(permalink: string) {
			return ticked.has(permalink);
		},

		/** Ticks one row or unticks it, and makes it what a Shift-click reaches from. */
		toggle(permalink: string) {
			pick([permalink], !ticked.has(permalink));
			anchor = permalink;
		},

		pick,

		/**
		 * A Shift-click: ticks every row between the one last ticked and this one,
		 * in the order the list is showing them, and leaves the rest alone. With
		 * nothing ticked yet — or an anchor the list no longer shows — there is
		 * nothing to reach from, so it ticks the one row.
		 *
		 * @param order the permalinks on screen, in the order shown.
		 * @param permalink the row clicked.
		 */
		extend(order: string[], permalink: string) {
			const from = anchor === null ? -1 : order.indexOf(anchor);
			const to = order.indexOf(permalink);

			if (from === -1 || to === -1) pick([permalink], true);
			else pick(order.slice(Math.min(from, to), Math.max(from, to) + 1), true);
			anchor = permalink;
		},

		/** Unticks everything, leaving the mode on. */
		clear: empty
	};
}
