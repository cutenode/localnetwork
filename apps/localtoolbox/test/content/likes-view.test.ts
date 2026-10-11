// content.ts — the one thing this extension asks a SoundCloud page to change
// about itself: /you/likes in its List view rather than its Badges grid.
//
// It is done by pressing SoundCloud's own "View" toggle, so what the page
// draws afterwards is SoundCloud's own doing and SoundCloud remembers the
// press. Which is also why it is off unless asked for, and why it presses
// once per arrival at the page: a person who presses Badges back must not
// have it taken from under them.
//
// The setting behind it is chrome.storage.local's `settings.likesListView`,
// which the popup's settings sheet writes (src/lib/settings.svelte.ts) and
// this reads; test/consistency.test.ts holds the key and the default together.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import { loadContentScript, disposeContentScripts } from '../helpers/load.ts';
import { createChrome, type ChromeMock, type Seed } from '../helpers/chrome.ts';
import { flush } from '../helpers/flush.ts';
import { render, ownLikesPage, playlistPage, playlistRow, likedSound, likedTile } from '../helpers/dom.ts';

declare const chrome: ChromeMock;

// The listeners a test puts on the document go when the test does: a toggle
// watched by an earlier test would otherwise go on redrawing the page for the
// later ones, including the one about a toggle that redraws nothing.
let listening: AbortController;

beforeEach(() => {
	vi.useFakeTimers();
	listening = new AbortController();
});

afterEach(() => {
	listening.abort();
	disposeContentScripts();
});

// The coalesced repaint, the same one the markers ride on: the observer's
// microtask, then the 150 ms a burst is bundled into.
const paint = async () => {
	await flush();
	vi.advanceTimersByTime(200);
	await flush();
};

const badgeToggle = () => document.querySelector<HTMLElement>('.listDisplayToggle__badgeToggle');

const showingGrid = () => !!document.querySelector('.collectionSection .badgeList');

// Counts the presses on both halves of the toggle. jsdom has no SoundCloud
// behind the button, so `redraws` is what SoundCloud would do with the press:
// the collection section comes back as the other layout. A test that leaves
// it off is a test about a toggle that answers nothing, which is what a
// renamed or reworked toggle would look like from here.
//
// The list it draws carries a purchase link, as a row does wherever the
// uploader set one: that is what the other setting acts on.
const PURCHASE = { url: 'https://hypeddit.com/x', label: 'Free Download' };

const watchToggle = ({ redraws = true } = {}) => {
	const presses = { list: 0, badges: 0 };

	const redraw = (list: boolean) => {
		const ul = document.querySelector('.collectionSection ul')!;
		ul.className = `${list ? 'soundList' : 'badgeList'} lazyLoadingList__list`;
		ul.innerHTML = list ? likedSound({ href: '/l/one', purchase: PURCHASE }) : likedTile({ href: '/t/one' });
	};

	document.addEventListener(
		'click',
		(event) => {
			if (event.target instanceof Element && event.target.closest('.listDisplayToggle__listToggle')) {
				presses.list += 1;

				if (redraws) redraw(true);
			}

			if (event.target instanceof Element && event.target.closest('.listDisplayToggle__badgeToggle')) {
				presses.badges += 1;

				if (redraws) redraw(false);
			}
		},
		{ signal: listening.signal }
	);

	return presses;
};

const badgesPage = () => ownLikesPage({ items: [likedTile({ href: '/t/one' })] });

const listPage = () => ownLikesPage({ list: true, items: [likedSound({ href: '/l/one' })] });

const openOn = async (html: string, { path = '/you/likes', storage }: { path?: string; storage?: Seed } = {}) => {
	globalThis.chrome = createChrome(storage ? { storage } : {});
	render(html, path);
	const presses = watchToggle();
	await loadContentScript();

	return presses;
};

const on = { storage: { settings: { likesListView: true } } };

describe('the Likes page’s view', () => {
	it('is left as SoundCloud drew it while the setting is off', async () => {
		const presses = await openOn(badgesPage());
		expect(presses).toEqual({ list: 0, badges: 0 });
		expect(showingGrid()).toBe(true);
	});

	it('is pressed into the List view when the setting is on', async () => {
		const presses = await openOn(badgesPage(), on);
		expect(presses).toEqual({ list: 1, badges: 0 });
		expect(showingGrid()).toBe(false);
	});

	// The toggle is only rendered once the Library layout is, which on a real
	// page is after this script starts.
	it('is pressed when the page arrives after the script does', async () => {
		globalThis.chrome = createChrome({ storage: on.storage });
		render('<div class="loading"></div>', '/you/likes');
		const presses = watchToggle();
		await loadContentScript();
		expect(presses.list).toBe(0);

		document.body.innerHTML = badgesPage();
		await paint();
		expect(presses.list).toBe(1);
	});

	it('presses nothing on a page that is a list already', async () => {
		const presses = await openOn(listPage(), on);
		expect(presses).toEqual({ list: 0, badges: 0 });
	});
});

// The press is a thing done to somebody's page, once, on arriving at it.
// Everything here is about it not happening twice.
describe('pressing it once', () => {
	it('does not press again for the repaints that follow', async () => {
		const presses = await openOn(badgesPage(), on);
		expect(presses.list).toBe(1);

		// Ten more liked items lazy-load into the list SoundCloud just drew.
		document.querySelector('.collectionSection ul')!.insertAdjacentHTML('beforeend', likedSound({ href: '/l/two' }));
		await paint();
		expect(presses.list).toBe(1);
	});

	it('leaves the grid alone when the toggle is pressed back by hand', async () => {
		const presses = await openOn(badgesPage(), on);
		expect(presses.list).toBe(1);

		badgeToggle()!.click();
		await paint();
		expect(presses).toEqual({ list: 1, badges: 1 });
		expect(showingGrid()).toBe(true);
	});

	// A toggle that answers nothing is the shape a renamed or reworked control
	// would have. One press is still all it gets — a page cannot be clicked at
	// every repaint because SoundCloud stopped listening.
	it('does not press again when the press changes nothing', async () => {
		globalThis.chrome = createChrome({ storage: on.storage });
		render(badgesPage(), '/you/likes');
		const presses = watchToggle({ redraws: false });
		await loadContentScript();
		expect(presses.list).toBe(1);

		document.querySelector('.collectionSection ul')!.insertAdjacentHTML('beforeend', likedTile({ href: '/t/two' }));
		await paint();
		expect(presses.list).toBe(1);
	});

	// SoundCloud is a single-page app: going somewhere else and coming back is
	// a new arrival at the page, and gets its own press.
	it('presses again after the page has been left and come back to', async () => {
		const presses = await openOn(badgesPage(), on);
		expect(presses.list).toBe(1);

		document.body.innerHTML = playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] });
		await paint();
		document.body.innerHTML = badgesPage();
		await paint();
		expect(presses.list).toBe(2);
	});
});

describe('the setting behind it', () => {
	it('presses as soon as it is turned on under an open page', async () => {
		const presses = await openOn(badgesPage());
		expect(presses.list).toBe(0);

		await chrome.storage.local.set({ settings: { likesListView: true } });
		await flush();
		expect(presses.list).toBe(1);
		expect(showingGrid()).toBe(false);
	});

	it('presses again after being turned off and on over a grid', async () => {
		const presses = await openOn(badgesPage(), on);
		expect(presses.list).toBe(1);

		badgeToggle()!.click();
		await paint();
		await chrome.storage.local.set({ settings: { likesListView: false } });
		await flush();
		expect(presses.list).toBe(1);

		await chrome.storage.local.set({ settings: { likesListView: true } });
		await flush();
		expect(presses.list).toBe(2);
	});

	it('stops pressing the moment it is turned off', async () => {
		const presses = await openOn(badgesPage(), on);
		expect(presses.list).toBe(1);

		await chrome.storage.local.set({ settings: { likesListView: false } });
		await flush();
		// Somewhere else, and back to the grid: a fresh arrival, which a setting
		// that is on would press for, and one that is off no longer does.
		document.body.innerHTML = playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] });
		await paint();
		document.body.innerHTML = badgesPage();
		await paint();
		expect(presses.list).toBe(1);
		expect(showingGrid()).toBe(true);
	});

	// Stored settings are not this version's to trust, and the one that is off
	// by default is the one where a wrong guess would press a button on
	// somebody's page unasked.
	it.each([
		['a value of the wrong type', { likesListView: 'yes' }],
		['a setting this version has never heard of', { somethingElse: 1 }],
		['something that is not an object at all', 'on']
	])('falls back to leaving the page alone, given %s', async (_what, settings) => {
		const presses = await openOn(badgesPage(), { storage: { settings } });
		expect(presses.list).toBe(0);
	});

	it('falls back to leaving the page alone when the setting cannot be read', async () => {
		globalThis.chrome = createChrome();
		const real = chrome.storage.local.get.getMockImplementation()!;
		chrome.storage.local.get.mockImplementation(async (keys) => {
			if (keys === 'settings') throw new Error('storage unavailable');

			return real(keys);
		});
		render(badgesPage(), '/you/likes');
		const presses = watchToggle();
		await loadContentScript();
		expect(presses.list).toBe(0);
	});

	// The two settings share one key and one object, so neither may read the
	// other's value or lose it.
	it('is read beside the other setting rather than instead of it', async () => {
		const presses = await openOn(badgesPage(), {
			storage: { settings: { likesListView: true, replaceDownloadButtons: false } }
		});

		expect(presses.list).toBe(1);
		// The other one was off, and stayed off through the press: the list the
		// press brought has a purchase link, and it is left as SoundCloud drew it.
		const purchase = document.querySelector('.soundActions__purchaseLink');
		expect(purchase).not.toBe(null);
		expect(purchase!.classList.contains('tbx-dl-replaced')).toBe(false);
		expect(document.querySelector('.tbx-dl')).toBe(null);
	});
});
