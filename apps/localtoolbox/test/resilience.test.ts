// What every panel does when the browser says no.
//
// A popup runs in a browser that is allowed to fail: storage can be full, a
// tab can have been closed between the query and the message, `localStorage`
// throws outright when site data is blocked, the clipboard needs a focused
// document and the popup loses focus to any file dialog, and a content script
// left over from before an update answers in a shape this version has never
// seen.
//
// Each of those is injected here against each panel that touches it. The bar
// is the same everywhere: draw something a person can read, leave no promise
// rejected on the floor, and — because a panel that cannot say why is as bad
// as one that crashed — keep the page accessible while doing it. "Something a
// person can read" is the words each panel has for what happened, not merely
// some text: a panel's own header is more than twenty characters, so a length
// was met by every panel whatever its body drew.
import { describe, it, expect, vi } from 'vitest';
import { audit } from './helpers/a11y.ts';
import {
	addPage,
	button,
	click,
	extraction,
	installClipboard,
	mountApp,
	mountPanel,
	openQueue,
	panelChrome,
	queueSheet,
	settle,
	text,
	track
} from './helpers/panel.ts';
import type { ChromeMock, Json } from './helpers/chrome.ts';
import type { Track } from '$lib/types';

declare const chrome: ChromeMock;

const TOOLS = ['soundcloud-playlist-to-text', 'manager', 'third-party-downloads', 'artists'] as const;

const TRACKS = [track('Bicep', 'Glue'), track('Four Tet', 'Baby')];

// The same two, looked up and each pointing at a gate: what the downloader
// needs to draw a row at all.
const gated = (one: Track, slug: string) => ({
	...one,
	details: {
		id: 1,
		permalink: one.permalink,
		downloadable: false,
		hasDownloadsLeft: true,
		purchaseUrl: `https://hypeddit.com/${slug}`,
		purchaseTitle: 'Free Download',
		at: 1
	}
});

const GATED = [gated(TRACKS[0], 'glue1'), gated(TRACKS[1], 'baby1')];

const answer = (tracks: object[] = TRACKS) => extraction({ tracks, lookupFailed: 0, resumed: false, error: '' });

// A rejection nobody handled fails the test it happened in — test/setup.js
// watches every test for one. All that is needed here is to give it a chance
// to arrive before the test ends, which is what `quiet()` does: rejections
// are delivered a macrotask after the promise settles.
const quiet = () => new Promise((resolve) => setImmediate(resolve));

const refuseWrites = () => {
	chrome.storage.local.set.mockRejectedValue(new Error('QUOTA_BYTES quota exceeded'));
	chrome.storage.session.set.mockRejectedValue(new Error('QUOTA_BYTES quota exceeded'));
};

const writes = () => chrome.storage.local.set.mock.calls.length + chrome.storage.session.set.mock.calls.length;

// Nothing here may leave the machine: a row's cover and its gate are both
// fetched when pressed.
const refuseFetches = () =>
	vi.stubGlobal(
		'fetch',
		vi.fn(async () => {
			throw new TypeError('Failed to fetch');
		})
	);

/** Mounts `id` and reads back what it said, and what an audit of it found unreadable. */
async function survive(id: string) {
	await mountPanel(id);
	await settle();
	await quiet();

	return { said: text(), unreadable: audit() };
}

/** A panel that survived: it said `says`, and readably. */
const survived = (says: string) => ({ said: expect.stringContaining(says), unreadable: [] });

// What each panel says with no tab to read. The downloader takes pasted links
// as well, so it says that; the two that keep lists say what an add needs.
const NO_TAB = {
	'soundcloud-playlist-to-text': 'open a SoundCloud playlist, likes or track page in this tab.',
	manager: 'not a SoundCloud playlist, likes or track page. open one to add it.',
	'third-party-downloads': 'open a gate link or a SoundCloud page in this tab, or paste links below.',
	artists: 'not a SoundCloud playlist, likes or track page. open one to add its artists.'
};

// Everything an older (or newer) content script might leave out, and what each
// panel makes of it: an answer it cannot read, a page with no tracks, or
// tracks it draws. The downloader reads a page for its gate links rather than
// its tracks, so tracks without one are nothing it knows — and it takes no
// answer at all for the page not being there.
const OTHER_VERSIONS = [
	{},
	{ ok: true },
	{ ok: true, tracks: null },
	{ ok: true, tracks: [] },
	{ ok: true, pageType: 'playlist', tracks: [{}] },
	{ ok: true, pageType: 'likes', tracks: [{ artist: null, title: null }] },
	{ ok: false },
	null,
	'not an object'
];

const R = 'couldn’t read the page. reload the tab and try again.';

const U = 'can’t reach the page — reload the tab.';

const N = 'that’s not a playlist, likes or track page.';

const K = 'nothing on SoundCloud points at a site this knows';

const T = '1 track';

const OF_OTHER_VERSIONS = {
	'soundcloud-playlist-to-text': [R, R, R, N, T, T, R, R, R],
	manager: [R, R, R, N, T, T, R, R, R],
	'third-party-downloads': [R, R, R, K, K, K, R, U, R],
	artists: [R, R, R, N, T, T, R, R, R]
};

// --- the browser's own failures ---------------------------------------------

describe.each(TOOLS)('%s', (id) => {
	// The page is still read and drawn: storage holds what a panel keeps, not
	// what it shows of the tab.
	it('survives storage that cannot be read', async () => {
		panelChrome({ response: answer() });
		chrome.storage.local.get.mockRejectedValue(new Error('storage unavailable'));
		chrome.storage.session.get.mockRejectedValue(new Error('storage unavailable'));
		expect(await survive(id), id).toEqual(survived('My Playlist'));
	});

	it('survives a tab that cannot be found', async () => {
		panelChrome({ response: answer() });
		chrome.tabs.query.mockRejectedValue(new Error('No tab with id'));
		expect(await survive(id), id).toEqual(survived(NO_TAB[id]));
	});

	it('survives a page that never answers', async () => {
		panelChrome({ response: new Error('Could not establish connection') });
		expect(await survive(id), id).toEqual(survived('can’t reach the page — reload the tab.'));
	});

	it('survives site data being blocked outright', async () => {
		panelChrome({ response: answer() });

		const throwing = {
			getItem: () => {
				throw new DOMException('The operation is insecure.', 'SecurityError');
			},
			setItem: () => {
				throw new DOMException('The operation is insecure.', 'SecurityError');
			},
			removeItem: () => {
				throw new DOMException('The operation is insecure.', 'SecurityError');
			},
			clear: () => {},
			key: () => null,
			length: 0
		};

		vi.stubGlobal('localStorage', throwing);
		expect(await survive(id), id).toEqual(survived('My Playlist'));
		vi.unstubAllGlobals();
	});

	it('survives an answer from a content script of another version', async () => {
		const says = OF_OTHER_VERSIONS[id];

		for (const [n, response] of OTHER_VERSIONS.entries()) {
			panelChrome({ response });
			expect(await survive(id), id).toEqual(survived(says[n]));
			document.body.innerHTML = '';
		}
	});
});

// The three panels that keep something of their own in extension storage, and
// the row each still draws once a write has been refused. The Manager and the
// artists tool fill lists of their own with an add, which is the first write;
// the downloader marks its rows and remembers what it walked. The playlist
// panel is not here: it keeps nothing in extension storage, so there is no
// write of its own to refuse.
describe.each([
	['manager', 'Glue'],
	['third-party-downloads', 'Bicep — Glue'],
	['artists', 'Four Tet']
])('%s', (id, row) => {
	it('survives storage that cannot be written', async () => {
		panelChrome({ response: answer(GATED) });
		refuseWrites();
		refuseFetches();
		expect(await survive(id), id).toEqual(survived('My Playlist'));

		if (id !== 'third-party-downloads') await addPage();

		// Every toggle a row offers, pressed against a storage that refuses.
		const controls = [...document.querySelectorAll('li button')].slice(0, 6);
		expect(controls.length, `${id} drew no row to press`).toBeGreaterThan(0);

		for (const control of controls) await click(control);
		await quiet();

		expect(writes(), 'nothing was written, so nothing was refused').toBeGreaterThan(0);
		// A refused write costs the storage, not the screen.
		expect(text()).toContain(row);
	});
});

// --- the queue sheet ---------------------------------------------------------

// Not one of the tools above: the sheet is drawn by the shell over whichever
// one is open, so it is put through the same failures against the whole popup.
// The queue it lists is session storage, which is exactly the storage these
// tests take away.
describe('the queue sheet', () => {
	/** Opens the popup and the sheet over it, and asserts both survived. */
	async function survivesOpen() {
		await mountApp();
		await openQueue();
		await settle();
		await quiet();
		const sheet = queueSheet();
		expect(sheet, 'the sheet did not open').not.toBe(null);
		expect(audit(), 'the sheet became unreadable').toEqual([]);

		return sheet!;
	}

	const rowsOf = (sheet: Element) => sheet.querySelectorAll('li');

	it('opens over a session storage that cannot be read', async () => {
		panelChrome({ response: answer() });
		chrome.storage.local.get.mockRejectedValue(new Error('storage unavailable'));
		chrome.storage.session.get.mockRejectedValue(new Error('storage unavailable'));
		const sheet = await survivesOpen();
		// Nothing to list, and it says so rather than drawing an empty sheet.
		expect(rowsOf(sheet)).toHaveLength(0);
		expect(sheet.textContent).toMatch(/nothing here yet/);
	});

	it('survives a queue it cannot write back', async () => {
		panelChrome({ response: answer(), session: { queue: TRACKS } });
		refuseWrites();
		const sheet = await survivesOpen();
		expect(rowsOf(sheet)).toHaveLength(2);
		expect(sheet.textContent).toContain('Glue');
		expect(sheet.textContent).toContain('Baby');

		// Marking and removing each, against a storage that refuses both.
		const controls = [...sheet.querySelectorAll('li button')];
		expect(controls).toHaveLength(4);

		for (const control of controls) await click(control);
		await quiet();

		expect(writes()).toBeGreaterThan(0);
		// The queue on screen is the one asked for, written or not.
		expect(rowsOf(queueSheet()!)).toHaveLength(0);
		expect(queueSheet()!.textContent).toMatch(/nothing here yet/);
		expect(audit()).toEqual([]);
	});

	// What is an entry is listed, and what is not is left out — and a queue
	// with nothing left in it says so rather than drawing an empty sheet.
	it('survives a queue left by a version that kept other fields', async () => {
		const queues: [queued: Json, rows: number, says: string][] = [
			[null, 0, 'nothing here yet'],
			['not an array', 0, 'nothing here yet'],
			[[{}], 0, 'nothing here yet'],
			[[{ permalink: '/x/y' }], 1, '1 track'],
			[[null], 0, 'nothing here yet']
		];

		for (const [queued, rows, says] of queues) {
			panelChrome({ response: answer(), session: { queue: queued } });
			const sheet = await survivesOpen();
			expect(rowsOf(sheet), JSON.stringify(queued)).toHaveLength(rows);
			expect(sheet.textContent, JSON.stringify(queued)).toContain(says);
			document.body.innerHTML = '';
		}
	});
});

// --- failures particular to one tool -----------------------------------------

describe('the clipboard', () => {
	it('falls back, and says so, when the async clipboard refuses', async () => {
		panelChrome({ response: answer() });
		const clipboard = installClipboard();
		clipboard.writeText.mockRejectedValue(new DOMException('Document is not focused'));
		// The old selection route is what is left, and jsdom has no execCommand.
		const execCommand = vi.fn<typeof document.execCommand>(() => false);
		document.execCommand = execCommand;

		await mountPanel('soundcloud-playlist-to-text');
		await click(button('copy'));
		await settle();
		await quiet();

		expect(clipboard.writeText).toHaveBeenCalled();
		expect(execCommand).toHaveBeenCalledWith('copy');
		expect(text()).toContain('couldn’t copy');
	});

	it('survives a browser with no clipboard API at all', async () => {
		panelChrome({ response: answer() });
		Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true });
		const execCommand = vi.fn<typeof document.execCommand>(() => true);
		document.execCommand = execCommand;
		await mountPanel('soundcloud-playlist-to-text');
		await click(button('copy'));
		await settle();
		await quiet();

		// The selection route is the whole of it here, and it worked.
		expect(execCommand).toHaveBeenCalledWith('copy');
		expect(text()).toContain('copied');
	});
});

describe('the cover art a row copies', () => {
	it('survives a fetch the browser refuses, and says so', async () => {
		// Checked, with nothing to offer: enough for the Manager panel to draw
		// the row, and with it the thumbnail this clicks.
		const checked = {
			...track('Bicep', 'Glue'),
			details: {
				id: 1,
				permalink: '/bicep/glue',
				downloadable: false,
				hasDownloadsLeft: true,
				purchaseUrl: '',
				purchaseTitle: '',
				at: 1
			}
		};

		panelChrome({ response: answer([checked]) });
		installClipboard();
		refuseFetches();

		await mountPanel('manager');
		await addPage();
		await click(document.querySelector('[aria-label="copy the cover art"]'));
		await settle();
		await quiet();

		expect(text()).toMatch(/couldn’t copy the cover/);
		expect(audit()).toEqual([]);
	});
});

describe('the network', () => {
	it('survives a fetch that never resolves into anything useful', async () => {
		panelChrome({
			response: answer([
				{
					...track('Artist', 'One'),
					details: {
						id: 1,
						permalink: '/artist/one',
						downloadable: false,
						hasDownloadsLeft: true,
						purchaseUrl: 'https://hypeddit.com/886rp1',
						purchaseTitle: '',
						at: 1
					}
				}
			])
		});
		refuseFetches();
		await mountPanel('third-party-downloads');
		await click(button('get all'));
		await settle();
		await quiet();

		expect(fetch).toHaveBeenCalled();
		// The row says what went wrong with it, and the run says how it went.
		expect(text()).toContain('Failed to fetch');
		expect(text()).toContain('saved 0 of 1 — check the rows that failed');
		expect(audit()).toEqual([]);
	});
});
