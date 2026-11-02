// src/lib/frame-height.svelte.ts — how tall a panel's frame may be.
import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest';

/** The observer the module starts, handed back so a test can say the content changed. */
let observed: (() => void) | null = null;

class FakeResizeObserver {
	constructor(callback: () => void) {
		observed = callback;
	}

	observe() {}
}

/** The document as tall as `content`, in a popup `popupHeight` tall. */
function popup(popupHeight: number, content: number) {
	vi.stubGlobal('innerHeight', popupHeight);
	Object.defineProperty(document.documentElement, 'scrollHeight', { configurable: true, value: content });
}

/** The popup stays as it is for long enough to be taken at its word. */
const settled = () => vi.advanceTimersByTime(200);

beforeEach(() => {
	vi.useFakeTimers();
});

afterEach(() => {
	Reflect.deleteProperty(document.documentElement, 'scrollHeight');
});

async function fresh() {
	vi.resetModules();
	observed = null;

	return (await import('../../src/lib/frame-height.svelte.ts')).frameHeight;
}

describe('the frame height', () => {
	it('is what fits under Chrome’s ceiling until the popup is seen to run out of room', async () => {
		vi.stubGlobal('ResizeObserver', FakeResizeObserver);
		const frame = await fresh();
		frame.start();
		expect(frame.max).toBe(580);

		// Content shorter than the popup: Chrome fitted the popup to it, and nothing comes down.
		popup(300, 300);
		observed!();
		settled();
		expect(frame.max).toBe(580);
	});

	it('comes down to the popup’s height when the content still overflows it once it has settled', async () => {
		vi.stubGlobal('ResizeObserver', FakeResizeObserver);
		const frame = await fresh();
		frame.start();

		// Zoomed to 200%: Chrome's 600 pixels are 300 CSS pixels, less its own frame.
		popup(249, 580);
		observed!();
		expect(frame.max).toBe(580);
		settled();
		expect(frame.max).toBe(249);

		// The popup being resized by Chrome is a chance to look again too.
		popup(200, 249);
		dispatchEvent(new Event('resize'));
		settled();
		expect(frame.max).toBe(200);
	});

	it('takes no notice of the frame or two the content outgrows the popup while Chrome catches up', async () => {
		vi.stubGlobal('ResizeObserver', FakeResizeObserver);
		const frame = await fresh();
		frame.start();

		// A panel filling in: past the popup for a moment, then Chrome grows the popup to fit.
		popup(244, 498);
		observed!();
		vi.advanceTimersByTime(50);
		popup(498, 498);
		dispatchEvent(new Event('resize'));
		settled();
		expect(frame.max).toBe(580);
	});

	it('starts once however often it is asked', async () => {
		let made = 0;
		vi.stubGlobal(
			'ResizeObserver',
			class extends FakeResizeObserver {
				constructor(callback: () => void) {
					super(callback);
					made++;
				}
			}
		);
		const frame = await fresh();
		frame.start();
		frame.start();
		expect(made).toBe(1);
	});

	it('stays at the ceiling where nothing can watch the popup', async () => {
		const frame = await fresh();
		expect(globalThis.ResizeObserver).toBeUndefined();
		frame.start();
		expect(frame.max).toBe(580);
	});
});
