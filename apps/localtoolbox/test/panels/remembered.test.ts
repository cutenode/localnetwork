// What a panel is still showing when the popup comes back.
//
// The popup is destroyed on every click that opens a tab, which working
// through a page is mostly made of. What survives that is the place someone
// was in — the search they typed, the drawer they opened, how far down the
// list they had got — kept per tool for the browsing session
// (src/lib/view.svelte.ts). What does not survive it is anything the page or a
// run owns; those are read again.
import { describe, it, expect, vi } from 'vitest';
import {
	addPage,
	click,
	extraction,
	mountPanel,
	panelChrome,
	reopenPanel,
	rows,
	settle,
	text,
	track,
	type
} from '../helpers/panel.ts';
import type { ChromeMock } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

const TRACKS = [
	track('Bicep', 'Glue'),
	track('Four Tet', 'Baby'),
	track('Overmono', 'So U Kno'),
	track('Joy Orbison', 'Flight Fm')
];

const search = () => document.querySelector<HTMLInputElement>('[aria-label="search the list"]')!;

const body = () => document.querySelector<HTMLElement>('.scroll-slim')!;

/**
 * jsdom takes any `scrollTop` it is given; a real browser clamps it to the
 * content there is, which is the whole reason the restore keeps trying as a
 * list loads. This puts the clamp back, with a height the test can grow.
 */
function clampScroll(max = 0) {
	const real = Object.getOwnPropertyDescriptor(Element.prototype, 'scrollTop');
	const scrolled = new WeakMap<Element, number>();

	const clamp = {
		max,
		restore: () => Object.defineProperty(Element.prototype, 'scrollTop', real!)
	};

	Object.defineProperty(Element.prototype, 'scrollTop', {
		configurable: true,
		get(this: Element) {
			return scrolled.get(this) ?? 0;
		},
		set(this: Element, value: number) {
			scrolled.set(this, Math.max(0, Math.min(value, clamp.max)));
		}
	});

	return clamp;
}

describe('the search over a list', () => {
	it('is still there, and still filtering, when the popup comes back', async () => {
		panelChrome({ response: extraction({ tracks: TRACKS }) });
		await mountPanel('manager');
		await addPage();
		await type(search(), 'bicep');
		expect(rows()).toHaveLength(1);

		await reopenPanel('manager');
		expect(search().value).toBe('bicep');
		expect(rows()).toHaveLength(1);
		expect(text()).toMatch(/Glue/);
	});

	// Kept under the tool's own id rather than one key for the whole popup, so
	// a search typed here is not waiting in whichever tool is opened next.
	it('is filed under the tool it was typed in', async () => {
		panelChrome({ response: extraction({ tracks: TRACKS }) });
		await mountPanel('manager');
		await addPage();
		await type(search(), 'bicep');
		await settle();

		const written = chrome.storage.session.set.mock.calls.at(-1)![0].view;

		// Under the manager's id, and nobody else's.
		expect(written).toHaveProperty('panels', { manager: expect.objectContaining({ query: 'bicep' }) });
	});

	it('is gone in a new browsing session, which starts on the whole list', async () => {
		panelChrome({ response: extraction({ tracks: TRACKS }) });
		await mountPanel('manager');
		await addPage();
		await type(search(), 'bicep');

		// A new session: the browser was closed, so its session storage went too.
		// The list is not session storage — it is kept for good, and is still
		// there, which is what the whole list coming back means here.
		const list = chrome.testStore('local').listed;
		panelChrome({ response: extraction({ tracks: TRACKS }), storage: { listed: list } });
		await reopenPanel('manager');
		expect(search().value).toBe('');
		expect(rows()).toHaveLength(4);
	});
});

describe('the Downloader’s drawer', () => {
	const drawer = () =>
		[...document.querySelectorAll('button')].find((b) => b.textContent.includes('add links by hand'));

	const paste = () => document.querySelector<HTMLTextAreaElement>('#paste');

	// By its exact text: "add links" would otherwise also match the drawer's own
	// trigger, "add links by hand".
	const addLinks = () =>
		[...document.querySelectorAll('button')].find((b) => b.textContent.replace(/\s+/g, ' ').trim() === 'add links');

	it('comes back open, with the links that were pasted into it', async () => {
		panelChrome({ tab: { id: 1, url: 'https://example.com/' } });
		await mountPanel('third-party-downloads');
		await click(drawer());
		await type(paste(), 'https://hypeddit.com/track/abc and some words');
		expect(paste()!.value).toMatch(/hypeddit/);

		await reopenPanel('third-party-downloads');
		expect(paste()).not.toBe(null);
		expect(paste()!.value).toBe('https://hypeddit.com/track/abc and some words');
	});

	it('is emptied by adding the links, and stays empty', async () => {
		panelChrome({ tab: { id: 1, url: 'https://example.com/' } });
		await mountPanel('third-party-downloads');
		await click(drawer());
		await type(paste(), 'https://hypeddit.com/track/abc');
		await click(addLinks());
		expect(paste()!.value).toBe('');

		await reopenPanel('third-party-downloads');
		expect(paste()!.value).toBe('');
	});

	// The one source that cannot be read again: no page has these links and no
	// queue holds them, so if the list does not put them back, nothing will.
	it('gives back the links that were added from it, whatever the tab is', async () => {
		panelChrome({ tab: { id: 1, url: 'https://example.com/' } });
		await mountPanel('third-party-downloads');
		await click(drawer());
		await type(paste(), 'https://hypeddit.com/track/abc https://droploud.com/gate/xyz');
		await click(addLinks());
		expect(rows()).toHaveLength(2);

		await reopenPanel('third-party-downloads');
		expect(rows()).toHaveLength(2);
		expect(text()).toMatch(/abc/);
		expect(text()).toMatch(/xyz/);
	});

	// It opens itself when there is nothing else to show, since it is the only
	// other place a link comes from — but not over and over.
	it('stays shut once it has been shut, however empty the panel is', async () => {
		panelChrome({ tab: { id: 1, url: 'https://example.com/' } });
		await mountPanel('third-party-downloads');
		expect(drawer()!.getAttribute('data-state')).toBe('open');

		await click(drawer());
		expect(drawer()!.getAttribute('data-state')).toBe('closed');

		await reopenPanel('third-party-downloads');
		expect(drawer()!.getAttribute('data-state')).toBe('closed');
	});
});

// The import box is the settings sheet's now, and what it remembers between
// openings of the popup is tested where it lives (test/saved-state.test.ts):
// the drawer comes back open, the replace switch comes back off, and what was
// pasted in is the one text box in the extension that is not kept.

describe('how far down the list someone had got', () => {
	const open = async () => {
		panelChrome({ response: extraction({ tracks: TRACKS }) });
		await mountPanel('soundcloud-playlist-to-text');
	};

	it('is where the list starts when the popup comes back', async () => {
		await open();
		body().scrollTop = 180;
		body().dispatchEvent(new Event('scroll'));
		await settle();

		await reopenPanel('soundcloud-playlist-to-text');
		expect(body().scrollTop).toBe(180);
	});

	it('follows the list back to the top', async () => {
		await open();
		body().scrollTop = 180;
		body().dispatchEvent(new Event('scroll'));
		await settle();
		body().scrollTop = 0;
		body().dispatchEvent(new Event('scroll'));
		await settle();

		await reopenPanel('soundcloud-playlist-to-text');
		expect(body().scrollTop).toBe(0);
	});

	it('keeps asking while the list is still arriving, and stops when it holds', async () => {
		await open();
		body().scrollTop = 180;
		body().dispatchEvent(new Event('scroll'));
		await settle();

		// Reopened onto a list that has not been drawn yet: nothing to scroll.
		const clamp = clampScroll(0);

		try {
			await reopenPanel('soundcloud-playlist-to-text');
			expect(body().scrollTop).toBe(0);

			// A partial list. The restore takes what it can get, and what it got is
			// its own doing, not a place to remember.
			clamp.max = 90;
			body().append(document.createElement('p'));
			await settle();
			body().dispatchEvent(new Event('scroll'));
			await settle();
			expect(body().scrollTop).toBe(90);

			// The rest of it arrives, and the position it was asked for holds.
			clamp.max = 400;
			body().append(document.createElement('p'));
			await settle();
			expect(body().scrollTop).toBe(180);
		} finally {
			clamp.restore();
		}

		await reopenPanel('soundcloud-playlist-to-text');
		expect(body().scrollTop).toBe(180);
	});

	it('leaves the list alone once the user has scrolled it somewhere else', async () => {
		await open();
		body().scrollTop = 180;
		body().dispatchEvent(new Event('scroll'));
		await settle();

		const clamp = clampScroll(400);

		try {
			await reopenPanel('soundcloud-playlist-to-text');
			// The user scrolls before the list is long enough for 180 to hold.
			clamp.max = 60;
			body().scrollTop = 60;
			body().dispatchEvent(new Event('scroll'));
			await settle();
			// Growing past 180 now does not drag the list back down to it.
			clamp.max = 400;
			body().append(document.createElement('p'));
			await settle();
			expect(body().scrollTop).toBe(60);
		} finally {
			clamp.restore();
		}

		await reopenPanel('soundcloud-playlist-to-text');
		expect(body().scrollTop).toBe(60);
	});

	// The restore and the user can both be moving the list at once. Where the
	// list is still too short for the place it is going back to, a scroll that
	// did not come from the restore is the user taking over: the restore stops
	// there, and the place they left it at is the one remembered.
	it('stops restoring the moment the user scrolls somewhere else', async () => {
		await open();
		body().scrollTop = 180;
		body().dispatchEvent(new Event('scroll'));
		await settle();

		const clamp = clampScroll(0);

		try {
			await reopenPanel('soundcloud-playlist-to-text');
			expect(body().scrollTop).toBe(0);

			// The list is nowhere near 180 yet, so the restore is still going when
			// the user scrolls to 40.
			clamp.max = 40;
			body().scrollTop = 40;
			body().dispatchEvent(new Event('scroll'));
			await settle();

			// The rest of the list arrives; 180 is reachable now and is not taken.
			clamp.max = 400;
			body().append(document.createElement('p'));
			await settle();
			expect(body().scrollTop).toBe(40);
		} finally {
			clamp.restore();
		}

		await reopenPanel('soundcloud-playlist-to-text');
		expect(body().scrollTop).toBe(40);
	});

	// A shorter page than the one it was left on: the list never grows enough to
	// reach the place, and nothing more arrives. The restore gives up after a
	// couple of seconds rather than dragging every later change — a row
	// finishing, a section opening — back down to the bottom.
	it('gives up on a list that never gets that long', async () => {
		vi.useFakeTimers();
		await open();
		body().scrollTop = 180;
		body().dispatchEvent(new Event('scroll'));
		await settle();

		const clamp = clampScroll(50);

		try {
			await reopenPanel('soundcloud-playlist-to-text');
			expect(body().scrollTop).toBe(50);

			await vi.advanceTimersByTimeAsync(2500);
			await settle();

			// Long enough now, and left alone: the restore is over.
			clamp.max = 400;
			body().append(document.createElement('p'));
			await settle();
			expect(body().scrollTop).toBe(50);
		} finally {
			clamp.restore();
		}
	});

	it('is the tool’s own, and starts at the top in a new session', async () => {
		await open();
		body().scrollTop = 180;
		body().dispatchEvent(new Event('scroll'));
		await settle();

		await reopenPanel('manager');
		expect(body().scrollTop).toBe(0);

		panelChrome({ response: extraction({ tracks: TRACKS }) });
		await reopenPanel('soundcloud-playlist-to-text');
		expect(body().scrollTop).toBe(0);
	});
});
