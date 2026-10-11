// src/lib/selection.svelte.ts — the tracks ticked in a list, and the mode that
// shows the tick boxes.
//
// A selection is kept for the browsing session (view.svelte.ts), so each test
// imports its own copy of both stores against its own chrome mock; `flush`
// covers the microtask the session writes are batched on.
import { describe, it, expect, vi } from 'vitest';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';
import { flush } from '../helpers/flush.ts';

declare const chrome: ChromeMock;

const TOOL = 'manager';

const ORDER = ['/a/one', '/b/two', '/c/three', '/d/four'];

/** A fresh selection for `TOOL`, over `session` as what the session holds. */
async function load(session = {}) {
	vi.resetModules();
	globalThis.chrome = createChrome({ session });
	const { view } = await import('../../src/lib/view.svelte.ts');
	await view.start();
	const { createSelection } = await import('../../src/lib/selection.svelte.ts');

	return { picks: createSelection(TOOL), view };
}

/** What the session holds for this tool, once the batched write has gone. */
const kept = async () => {
	await flush();

	return chrome.testStore('session').view?.panels?.[TOOL];
};

describe('the mode', () => {
	it('starts off, with nothing ticked', async () => {
		const { picks } = await load();
		expect(picks.active).toBe(false);
		expect(picks.count).toBe(0);
		expect(picks.permalinks).toEqual([]);
		expect(picks.anchor).toBe(null);
		expect(picks.has('/a/one')).toBe(false);
	});

	it('is kept for the session, with the ticks it was left with', async () => {
		const { picks } = await load();
		picks.active = true;
		picks.toggle('/b/two');
		expect(await kept()).toEqual({ selecting: true, picked: ['/b/two'] });

		// What a popup opened again on the same browsing session comes back to.
		const { picks: reopened } = await load({
			view: { open: TOOL, panels: { [TOOL]: { selecting: true, picked: ['/b/two'] } } }
		});

		expect(reopened.active).toBe(true);
		expect(reopened.count).toBe(1);
		expect(reopened.has('/b/two')).toBe(true);
		// The anchor is half of a gesture, not a place: it does not come back.
		expect(reopened.anchor).toBe(null);
	});

	it('unticks everything when it is turned off', async () => {
		const { picks } = await load();
		picks.active = true;
		picks.pick(ORDER, true);
		expect(picks.count).toBe(4);

		picks.active = false;
		expect(picks.count).toBe(0);
		expect(picks.anchor).toBe(null);
		expect(await kept()).toEqual({ selecting: false, picked: [] });
	});
});

describe('ticking rows', () => {
	it('ticks one row and unticks it, and remembers where the click was', async () => {
		const { picks } = await load();
		picks.toggle('/b/two');
		expect(picks.has('/b/two')).toBe(true);
		expect(picks.anchor).toBe('/b/two');

		picks.toggle('/b/two');
		expect(picks.has('/b/two')).toBe(false);
		expect(picks.count).toBe(0);
		expect(picks.anchor).toBe('/b/two');
	});

	it('ticks a whole set at once, and unticks one', async () => {
		const { picks } = await load();
		picks.pick(['/a/one', '/c/three'], true);
		expect(picks.permalinks).toEqual(['/a/one', '/c/three']);

		// Already ticked: a row is ticked once however often it is named.
		picks.pick(['/a/one'], true);
		expect(picks.permalinks).toEqual(['/a/one', '/c/three']);

		picks.pick(['/a/one', '/never/ticked'], false);
		expect(picks.permalinks).toEqual(['/c/three']);
	});

	it('passes over a row with no permalink to work by', async () => {
		const { picks } = await load();
		// @ts-expect-error -- a row with no permalink at all, beside one with an empty one
		picks.pick(['', undefined, '/a/one'], true);
		expect(picks.permalinks).toEqual(['/a/one']);
	});

	it('unticks everything, leaving the mode on', async () => {
		const { picks } = await load();
		picks.active = true;
		picks.pick(ORDER, true);
		picks.clear();
		expect(picks.active).toBe(true);
		expect(picks.count).toBe(0);
		expect(picks.anchor).toBe(null);
	});
});

describe('a Shift-click', () => {
	it('reaches from the row last ticked to the one clicked', async () => {
		const { picks } = await load();
		picks.toggle('/a/one');
		picks.extend(ORDER, '/c/three');
		expect(picks.permalinks).toEqual(['/a/one', '/b/two', '/c/three']);
		// And the row clicked is what the next one reaches from.
		expect(picks.anchor).toBe('/c/three');
	});

	it('reaches upwards just as well', async () => {
		const { picks } = await load();
		picks.toggle('/d/four');
		picks.extend(ORDER, '/b/two');
		expect(picks.permalinks.sort()).toEqual(['/b/two', '/c/three', '/d/four']);
	});

	it('only ever ticks: the rows it reaches over that are ticked stay ticked', async () => {
		const { picks } = await load();
		picks.pick(['/b/two'], true);
		picks.toggle('/a/one');
		picks.extend(ORDER, '/c/three');
		expect(picks.permalinks).toEqual(['/b/two', '/a/one', '/c/three']);
	});

	it('ticks the one row when there is nothing to reach from', async () => {
		const { picks } = await load();
		picks.extend(ORDER, '/c/three');
		expect(picks.permalinks).toEqual(['/c/three']);
		expect(picks.anchor).toBe('/c/three');
	});

	it('ticks the one row when the list no longer shows the anchor', async () => {
		const { picks } = await load();
		picks.toggle('/a/one');
		// The filter has since hidden the row that was ticked.
		picks.extend(['/c/three', '/d/four'], '/d/four');
		expect(picks.permalinks).toEqual(['/a/one', '/d/four']);
	});

	it('ticks just the row clicked when the list does not show it', async () => {
		const { picks } = await load();
		picks.toggle('/a/one');
		picks.extend(ORDER, '/elsewhere/x');
		expect(picks.permalinks).toEqual(['/a/one', '/elsewhere/x']);
		expect(picks.anchor).toBe('/elsewhere/x');
	});
});
