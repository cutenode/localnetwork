// src/App.svelte and the tool menu it opens on.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
	click,
	clickNow,
	extraction,
	mountApp,
	queueButton,
	panelChrome,
	settle,
	settleUntil,
	SOUNDCLOUD_TAB,
	text
} from './helpers/panel.ts';
import { TOOLS } from '../src/lib/tools.ts';
import type { ChromeMock } from './helpers/chrome.ts';

declare const chrome: ChromeMock;

const menuButtons = () => [...document.querySelectorAll<HTMLButtonElement>('nav button')];

const names = () => menuButtons().map((b) => b.querySelector('span > span')!.textContent.trim());

const heading = () => document.querySelector('h1')!.textContent.trim();

const brand = () => document.documentElement.style.getPropertyValue('--brand');

const toolBrand = (id: string) => TOOLS.find((tool) => tool.id === id)!.brand;

// Last in the header, whatever else is in it: the queue button sits in front
// of the theme toggle, and panels put their own actions there too.
const themeButton = () => [...document.querySelectorAll('header button')].at(-1);

beforeEach(() => {
	panelChrome({ tab: { id: 1, url: 'https://example.com/' } });
});

describe('the tool menu', () => {
	it('lists every tool with its description and its own colour', async () => {
		await mountApp();
		expect(names()).toEqual(['SoundCloud playlist to text', 'Manager', 'third-party downloader', 'artists']);

		for (const button of menuButtons()) {
			expect(button.type).toBe('button');
			expect(button.textContent).toMatch(/\w.*\./);
			expect(button.style.getPropertyValue('--brand')).toMatch(/^oklch\(/);
		}

		// The two SoundCloud page tools both read a playlist, likes or track page.
		for (const button of menuButtons().slice(0, 2)) {
			expect(button.textContent).toMatch(/playlist, likes (or|and) track page/);
		}
	});

	it('opens a tool, paints the popup in its colour, and comes back', async () => {
		await mountApp();
		const menuBrand = brand();
		expect(heading()).toBe('localtoolbox');

		await click(menuButtons()[1]);
		expect(heading()).toBe('Manager');
		expect(brand()).not.toBe(menuBrand);
		expect(brand()).toMatch(/^oklch\(/);

		await click(document.querySelector('[aria-label="back to all tools"]'));
		expect(heading()).toBe('localtoolbox');
		expect(brand()).toBe(menuBrand);
	});

	it('opens each of the tools', async () => {
		for (const [index, tool] of TOOLS.entries()) {
			// A fresh browsing session each time: the popup comes back where it was
			// left, so the tool opened last time would otherwise be open already.
			panelChrome({ tab: { id: 1, url: 'https://example.com/' } });
			await mountApp();
			await click(menuButtons()[index]);
			// The panel itself, not the ghost that stands in while it loads — the
			// ghost has the tool's heading too.
			await settleUntil(() => document.querySelector('[aria-busy="true"]') === null);
			expect(document.querySelector('[aria-busy="true"]'), tool.id).toBe(null);
			expect(heading()).toBe(tool.name);
			document.body.innerHTML = '';
		}
	});
});

// The popup is destroyed on every click that opens a tab, which in these tools
// is most clicks. It is the same browsing session, so it comes back to it.
describe('where the popup was left', () => {
	/** Closes the popup and opens it again, as clicking away and back does. */
	const reopen = async () => {
		document.body.innerHTML = '';
		await mountApp();
	};

	it('opens on the tool that was open, not on the menu', async () => {
		await mountApp();
		await click(menuButtons()[1]);
		expect(heading()).toBe('Manager');

		await reopen();
		expect(heading()).toBe('Manager');
		expect(brand()).toBe(toolBrand('manager'));
	});

	it('opens on the menu again once the way back has been taken', async () => {
		await mountApp();
		await click(menuButtons()[2]);
		await click(document.querySelector('[aria-label="back to all tools"]'));
		expect(heading()).toBe('localtoolbox');

		await reopen();
		expect(heading()).toBe('localtoolbox');
	});

	it('opens on the menu when the session names a tool this version has not got', async () => {
		panelChrome({
			tab: { id: 1, url: 'https://example.com/' },
			session: { view: { open: 'soundcloud-lyrics', panels: {} } }
		});
		await mountApp();
		expect(heading()).toBe('localtoolbox');
	});

	it('opens on the menu when there is no session storage to read', async () => {
		panelChrome({ tab: { id: 1, url: 'https://example.com/' } });
		chrome.storage.session.get.mockRejectedValue(new Error('no session storage'));
		await mountApp();
		expect(heading()).toBe('localtoolbox');
		expect(names()).toHaveLength(4);
	});
});

// A panel is its own chunk, and the session it restores from is a browser
// API away, so between picking a tool and seeing it there is a moment the
// popup has to draw something for. What it draws is the tool's own header
// over a ghost of its body — and the header is drawn from the localStorage
// mirror of the route, which needs nothing awaited at all.
describe('opening a tool that is not there yet', () => {
	const ghost = () => document.querySelector('[aria-busy="true"]');
	// The view being replaced is still in the DOM while it slides out, and it
	// has a heading of its own; this is the one on the frame going up.
	const ghostHeading = () => ghost()?.querySelector('h1')!.textContent.trim();

	it('draws the tool’s frame, and a way out of it, while its chunk loads', async () => {
		await mountApp({ chunks: false });
		// The frame on the very next frame: an import cannot come back this turn.
		clickNow(menuButtons()[1]);

		expect(ghostHeading()).toBe('Manager');
		expect(brand()).toBe(toolBrand('manager'));
		expect(text()).toMatch(/opening…/);

		// Waiting for a chunk is exactly when someone changes their mind.
		await click(document.querySelector('[aria-label="back to all tools"]'));
		expect(heading()).toBe('localtoolbox');
		expect(ghost()).toBe(null);
	});

	it('starts reading the page before the panel that wants it exists', async () => {
		const browser = panelChrome({ tab: SOUNDCLOUD_TAB, response: extraction() });
		await mountApp({ chunks: false });
		clickNow(menuButtons()[0]);

		// Still a ghost, and the tab has already been asked for: the read the
		// panel will want is under way before the panel has been fetched.
		expect(ghost()).not.toBe(null);
		expect(browser.tabs.query).toHaveBeenCalled();

		// And when the panel does arrive — which is a real dynamic import, so it
		// is waited for rather than assumed — it is handed that read rather than
		// starting a second one.
		await settleUntil(() => ghost() === null);
		expect(ghost()).toBe(null);
		expect(browser.tabs.query).toHaveBeenCalledTimes(1);
		expect(browser.tabs.sendMessage).toHaveBeenCalledTimes(1);
		expect(browser.tabs.sendMessage).toHaveBeenCalledWith(SOUNDCLOUD_TAB.id, {
			type: 'EXTRACT_TRACKS'
		});
	});

	it('draws the tool it was left on before the session has answered', async () => {
		panelChrome({ tab: { id: 1, url: 'https://example.com/' } });
		// The session read that never comes back: what is on screen until it does
		// is what the popup opens with, every time it is opened.
		chrome.storage.session.get.mockReturnValue(new Promise(() => {}));
		localStorage.setItem('view:open', 'third-party-downloads');

		await mountApp();
		expect(ghostHeading()).toBe('third-party downloader');
		expect(heading()).toBe('third-party downloader');
		expect(brand()).toBe(toolBrand('third-party-downloads'));
	});

	// The queue behind the count in that header is another round trip, and it
	// has a mirror for the same reason the route does: without one the badge was
	// missing for a round trip on every single opening, for a queue that had not
	// moved since the last one.
	it('carries the count of the queue it was left with, before the session has answered', async () => {
		chrome.storage.session.get.mockReturnValue(new Promise(() => {}));
		localStorage.setItem('queue:items', JSON.stringify([{ permalink: '/a/one' }, { permalink: '/b/two' }]));

		await mountApp();
		const badge = queueButton()!.querySelector('span[aria-hidden="true"]')!;
		expect(badge.textContent.trim()).toBe('2');
		expect(queueButton()!.getAttribute('aria-label')).toMatch(/\b2 tracks\b/);
	});

	it('opens on the menu when the mirror names a tool the session does not', async () => {
		// A new browsing session: localStorage outlives one, the session does not.
		localStorage.setItem('view:open', 'manager');
		panelChrome({ tab: { id: 1, url: 'https://example.com/' } });

		await mountApp();
		expect(heading()).toBe('localtoolbox');
		expect(names()).toHaveLength(4);
		// And the mirror is corrected, so the next opening does not guess again.
		expect(localStorage.getItem('view:open')).toBe(null);
	});

	it('says so, and stays usable, when a panel’s chunk cannot be loaded', async () => {
		await mountApp({ chunks: 'broken' });
		await click(menuButtons()[1]);

		expect(text()).toMatch(/couldn’t open Manager/);
		expect(document.querySelector('[role="alert"]')).not.toBe(null);
		// The frame is still there, and so is the way out of it.
		await click(document.querySelector('[aria-label="back to all tools"]'));
		expect(heading()).toBe('localtoolbox');
		expect(text()).not.toMatch(/couldn’t open/);
	});

	// The sheet is a chunk of its own, for the same reason the panels are, and
	// it can go missing the same way — mid-update, with the popup open.
	it('says so when the queue sheet’s chunk cannot be loaded', async () => {
		panelChrome({
			tab: { id: 1, url: 'https://example.com/' },
			session: { queue: [{ permalink: '/b/g', title: 'Glue', artist: 'Bicep', artworkUrl: '' }] }
		});
		// The sheet is fetched as soon as the session says the queue has something
		// in it, so the session waits until the mounted popup's chunk is broken.
		let answer = () => {};

		const asked = new Promise<void>((resolve) => {
			answer = resolve;
		});

		const read = chrome.storage.session.get.getMockImplementation()!;

		chrome.storage.session.get.mockImplementation(async (keys) => {
			await asked;

			return read(keys);
		});

		await mountApp({ chunks: false });
		// mountApp loads the app afresh, so this is the module the popup just imported.
		const { sheetChunk } = await import('../src/lib/queue-sheet.ts');
		sheetChunk.load = () => Promise.reject(new Error('chunk missing'));
		answer();
		await settleUntil(() => /couldn’t open the queue/.test(text()));

		expect(text()).toMatch(/couldn’t open the queue/);
		expect(document.querySelector('[role="alert"]')).not.toBe(null);
	});

	it('keeps the mirror in step with the tool that is open', async () => {
		await mountApp();
		expect(localStorage.getItem('view:open')).toBe(null);

		await click(menuButtons()[2]);
		expect(localStorage.getItem('view:open')).toBe('third-party-downloads');

		await click(document.querySelector('[aria-label="back to all tools"]'));
		expect(localStorage.getItem('view:open')).toBe(null);
	});
});

// The queue is the one thing that opens a tool without going through the
// menu; that, and what it hands the tool, is in test/queue-sheet.test.ts
// ("handing the batch to another tool").

describe('the theme', () => {
	const isDark = () => document.documentElement.classList.contains('dark');

	it('follows the browser until it is told otherwise, then cycles', async () => {
		await mountApp();
		expect(isDark()).toBe(false);
		expect(text()).toMatch(/toolbox/);

		await click(themeButton());
		expect(localStorage.getItem('pref:theme')).toBe('light');
		expect(isDark()).toBe(false);

		await click(themeButton());
		expect(localStorage.getItem('pref:theme')).toBe('dark');
		expect(isDark()).toBe(true);

		await click(themeButton());
		expect(localStorage.getItem('pref:theme')).toBe('system');
		expect(isDark()).toBe(false);
	});

	it('restores the choice from last time', async () => {
		localStorage.setItem('pref:theme', 'dark');
		await mountApp();
		expect(isDark()).toBe(true);
	});

	it('ignores a stored value that is not a mode', async () => {
		localStorage.setItem('pref:theme', 'neon');
		await mountApp();
		expect(isDark()).toBe(false);
		expect(document.querySelector('[aria-label^="theme:"]')!.getAttribute('aria-label')).toBe(
			'theme: follows the browser'
		);
	});

	it('follows the browser switching to dark while the popup is open', async () => {
		const listeners: ((event: { matches: boolean }) => void)[] = [];
		// What the popup reads of a MediaQueryList: whether it matches, and its change event.
		vi.stubGlobal('matchMedia', (query: string) => ({
			matches: false,
			media: query,
			addEventListener: (_type: string, fn: (typeof listeners)[number]) => listeners.push(fn),
			removeEventListener: () => {}
		}));
		await mountApp();
		expect(isDark()).toBe(false);

		for (const fn of listeners) fn({ matches: true });
		await settle();
		expect(isDark()).toBe(true);
	});

	it('copes with a browser that has no matchMedia, and with unwritable storage', async () => {
		// @ts-expect-error -- a browser with no matchMedia; the DOM types never lack one
		delete globalThis.matchMedia;
		const real = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
		Object.defineProperty(globalThis, 'localStorage', {
			configurable: true,
			get() {
				throw new Error('denied');
			}
		});

		try {
			await mountApp();
			expect(isDark()).toBe(false);
			await click(themeButton());
			expect(isDark()).toBe(false);
		} finally {
			Object.defineProperty(globalThis, 'localStorage', real!);
		}
	});
});
