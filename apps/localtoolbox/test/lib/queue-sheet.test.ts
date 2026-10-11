// src/lib/queue-sheet.ts — fetching the queue sheet's chunk.
//
// The sheet is not in the tool registry (it is not a tool and has no route),
// so it has its own loader rather than `loadPanel`. What it owes the shell is
// what a panel's chunk owes it: one fetch however often the sheet is opened,
// and a failure that can be tried again.
import { describe, it, expect, vi } from 'vitest';

const load = async () => {
	vi.resetModules();

	return import('../../src/lib/queue-sheet.ts');
};

describe('loadQueueSheet', () => {
	it('fetches the chunk once, however often the sheet is opened', async () => {
		const { loadQueueSheet } = await load();

		const first = loadQueueSheet();
		// The same promise, not a second fetch: closing the sheet and opening it
		// again loads nothing.
		expect(loadQueueSheet()).toBe(first);
		expect(await first).toBeTypeOf('function');
		await expect(loadQueueSheet()).resolves.toBe(await first);
	});

	// The file is inside the extension, so this is a half-written update on
	// disk rather than a network failure — and the queue is the session's work,
	// which is too much to lose to one bad fetch.
	it('does not remember a chunk that failed to arrive', async () => {
		const { loadQueueSheet, sheetChunk } = await load();
		const real = sheetChunk.load;
		let asks = 0;
		sheetChunk.load = () => {
			asks += 1;

			return asks === 1 ? Promise.reject(new Error('chunk missing')) : real();
		};

		await expect(loadQueueSheet()).rejects.toThrow('chunk missing');

		// Nothing was cached, so pressing the button again really tries again.
		expect(await loadQueueSheet()).toBeTypeOf('function');
		expect(asks).toBe(2);
	});
});
