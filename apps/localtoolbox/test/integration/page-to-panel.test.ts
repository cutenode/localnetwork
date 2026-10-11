// The two halves of the extension, wired together.
//
// Everywhere else in the suite one half is a fixture: a panel is given an
// answer that looks like the content script's, and the content script is asked
// for one by a test rather than by a panel. Both fixtures can be right about
// the shape and wrong about the same thing, and nothing would notice.
//
// Here nothing is a fixture. Real SoundCloud markup goes on the page, the real
// content script reads it, `chrome.tabs.sendMessage` carries the answer to a
// real panel, and the assertions are on what the panel draws. The storage
// mock is shared, so the two contexts see each other's writes the way they do
// in the browser: a mark made in the popup paints a marker on the page, and a
// download started on the page lights up the row in the popup.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { createChrome, type ChromeMock, type Seed } from '../helpers/chrome.ts';
import { loadContentScript, disposeContentScripts } from '../helpers/load.ts';
import { mockFetch } from '../helpers/api.ts';
import { flush } from '../helpers/flush.ts';
import {
	render,
	playlistPage,
	playlistRow,
	publicLikesPage,
	likedSound,
	trackPage,
	hydrationScript,
	apiClientEntry,
	apiTrack,
	moreMenu,
	ART,
	type TrackPageOptions
} from '../helpers/dom.ts';
import {
	SOUNDCLOUD_TAB,
	addPage,
	button,
	click,
	installClipboard,
	mountApp,
	mountPanel,
	openQueue,
	rows,
	settle,
	text
} from '../helpers/panel.ts';

declare const chrome: ChromeMock;

const SET = '/curator/sets/my-playlist';

beforeEach(() => {
	vi.useFakeTimers();
});

afterEach(disposeContentScripts);

/**
 * Puts a page on screen and starts the real content script over it, with a
 * chrome whose `tabs.sendMessage` delivers to that script — which is what the
 * browser does when the popup messages the active tab.
 */
async function openPage(
	html: string,
	path = SET,
	{ storage = {}, session = {} }: { storage?: Seed; session?: Seed } = {}
) {
	globalThis.chrome = createChrome({
		tab: { ...SOUNDCLOUD_TAB, url: `https://soundcloud.com${path}` },
		storage,
		session
	});
	chrome.tabs.sendMessage.mockImplementation(async (_tabId, message) => chrome.testSendMessage(message));
	render(html, path);

	return loadContentScript();
}

/** Lets the content script's debounced marker pass run. */
const repaint = async () => {
	await vi.advanceTimersByTimeAsync(400);
	await flush();
};

// What the panel says, read off the panel alone. `text()` reads the whole
// document, and here that is the SoundCloud page as well — which carries the
// playlist's title, its curator and its tracks' names in its own markup.
const panelMeta = () => document.querySelector('header p')!.textContent.trim();

const panelBody = () => document.querySelector('.scroll-slim')!.textContent.replace(/\s+/g, ' ');

const markers = () =>
	[...document.querySelectorAll<HTMLElement>('.tbx-mark')].map((mark) => ({
		kind: mark.dataset.kind,
		row: mark.closest('.trackItem, .sound')?.querySelector('.trackItem__trackTitle, .soundTitle__title')?.textContent
	}));

// ---------------------------------------------------------------------------

describe('a playlist page read by the Playlist to Text panel', () => {
	it('turns the rows on the page into the text the panel copies', async () => {
		await openPage(
			playlistPage({
				title: 'Late Night',
				curator: 'Curator',
				rows: [
					playlistRow({ artist: 'Bicep', title: 'Glue', href: `/bicep/glue?in=curator/sets/my-playlist` }),
					playlistRow({ artist: 'Four Tet', title: 'Baby', href: `/fourtet/baby?in=curator/sets/my-playlist` }),
					playlistRow({ artist: 'Floating Points', title: 'LesAlpx', href: `/fp/lesalpx?in=curator/sets/my-playlist` })
				]
			})
		);
		const clipboard = installClipboard();
		await mountPanel('soundcloud-playlist-to-text');

		// The page's own words, having gone through the content script.
		expect(panelMeta()).toBe('Late Night · by Curator · 3 tracks');

		await click(button('copy'));
		expect(clipboard.writeText).toHaveBeenCalledWith('Bicep - Glue\nFour Tet - Baby\nFloating Points - LesAlpx');
	});

	it('carries a Likes page and its skipped playlists through to the panel', async () => {
		await openPage(
			publicLikesPage({
				user: 'dj-someone',
				heading: 'Likes by DJ Someone',
				items: [
					likedSound({ artist: 'Bicep', title: 'Glue', href: '/bicep/glue', artwork: ART('one', 't120x120') }),
					likedSound({ title: 'A Liked Playlist', playlist: true })
				]
			}),
			'/dj-someone/likes'
		);
		await mountPanel('soundcloud-playlist-to-text');

		expect(panelMeta()).toBe('Likes by DJ Someone · 1 track · 1 liked playlist skipped');
	});

	it('says the page has nothing on it in the panel’s own words', async () => {
		await openPage('<main><p>a profile</p></main>', '/monstercat');
		await mountPanel('soundcloud-playlist-to-text');
		expect(text()).toMatch(/not a playlist, likes or track page/);
	});
});

// ---------------------------------------------------------------------------

describe('a song page read by the Manager panel', () => {
	const TRACK = '/bicep/glue';

	const openSong = async (options: TrackPageOptions = {}) => {
		await openPage(
			trackPage({
				uploader: 'bicep',
				uploaderName: 'Bicep',
				title: 'Glue',
				genre: 'Techno',
				liked: true,
				artwork: ART('hero', 't500x500'),
				...options
			}),
			TRACK
		);
	};

	// The whole of it, end to end: the hero on the page, the content script
	// reading it, and one row on the list — with the details taken out of the
	// page's own hydration, so SoundCloud is not asked anything at all.
	it('puts the one track the page is about on the list, answers and all', async () => {
		await openSong();
		hydrationScript([
			apiClientEntry(),
			{
				hydratable: 'sound',
				data: apiTrack({ id: 5, permalink: TRACK, downloadable: true, genre: 'Techno' })
			}
		]);
		const api = mockFetch();
		disposeContentScripts();
		await loadContentScript();

		await mountPanel('manager');
		expect(panelBody()).toMatch(/Glue by Bicep · 1 track/);

		await addPage();
		await vi.advanceTimersByTimeAsync(1000);
		await settle();

		expect(rows()).toHaveLength(1);
		const row = document.querySelector(`li[data-permalink="${TRACK}"]`);
		expect(row).not.toBeNull();
		expect(row!.textContent).toContain('Glue');
		expect(row!.textContent).toContain('Bicep');
		// The hydration answered for it, so nothing went out to SoundCloud.
		expect(api.calls).toEqual([]);
		expect(chrome.testStore('local').listed[0]).toMatchObject({
			permalink: TRACK,
			title: 'Glue',
			artist: 'Bicep',
			genre: 'Techno',
			liked: true,
			from: 'Glue'
		});
	});

	// The purchase link a track page renders in its engagement bar is an answer
	// the page gives for free, exactly as a Likes row's is.
	it('carries the purchase link the page renders through to the row', async () => {
		await openSong({ purchase: { url: 'https://hypeddit.com/x/y', label: 'Free Download' } });
		hydrationScript([apiClientEntry()]);
		mockFetch();
		disposeContentScripts();
		await loadContentScript();

		await mountPanel('manager');
		await addPage();
		await vi.advanceTimersByTimeAsync(1000);
		await settle();

		expect(text()).toContain('hypeddit.com');
	});
});

// ---------------------------------------------------------------------------

describe('marks travelling between the page and the popup', () => {
	const page = () =>
		playlistPage({
			rows: [
				playlistRow({ artist: 'Bicep', title: 'Glue', href: `/bicep/glue?in=curator/sets/my-playlist` }),
				playlistRow({ artist: 'Four Tet', title: 'Baby', href: `/fourtet/baby?in=curator/sets/my-playlist` })
			]
		});

	const openWithDetails = async (storage: Seed = {}) => {
		await openPage(page(), SET, { storage });
		hydrationScript([
			apiClientEntry(),
			{
				hydratable: 'playlist',
				data: {
					kind: 'playlist',
					permalink_url: `https://soundcloud.com${SET}`,
					tracks: [
						apiTrack({ id: 1, permalink: '/bicep/glue', downloadable: true }),
						apiTrack({ id: 2, permalink: '/fourtet/baby', purchaseUrl: 'https://hypeddit.com/x/y' })
					]
				}
			}
		]);
		mockFetch();
		// The script read the page before the hydration was appended; a fresh one
		// starts up with it, as it would on a real load.
		disposeContentScripts();
		await loadContentScript();
	};

	it('paints a marker on the page when the popup marks a track', async () => {
		await openWithDetails();
		await mountPanel('manager');
		await addPage();
		await vi.advanceTimersByTimeAsync(1000);
		await settle();

		expect(markers()).toEqual([]);

		const mark = document.querySelector('li[data-permalink="/bicep/glue"] button[aria-label="mark downloaded"]');
		await click(mark);
		await repaint();

		// The content script, in the page, saw the popup's write.
		expect(markers()).toEqual([{ kind: 'downloaded', row: 'Glue' }]);
		expect(Object.keys(chrome.testStore().downloaded)).toEqual(['/bicep/glue']);
	});

	// The thumbnail the page rendered is the row's handle on the full-size
	// artwork: the address is rewritten from the one the content script read,
	// so nothing is requested to find it.
	it('opens the full-size cover art derived from the thumbnail the page showed', async () => {
		await openWithDetails();
		await mountPanel('manager');
		await addPage();
		await vi.advanceTimersByTimeAsync(1000);
		await settle();

		const row = document.querySelector('li[data-permalink="/bicep/glue"]')!;
		expect(row.querySelector('img')!.src).toBe(ART('row'));

		await click(row.querySelector('[aria-label="copy the cover art"]'), { metaKey: true });
		expect(chrome.tabs.create).toHaveBeenCalledWith({
			url: ART('row', 't1080x1080'),
			active: false
		});
	});

	it('lights up the popup’s row when the download is started on the page', async () => {
		await openWithDetails();
		await mountPanel('manager');
		await addPage();
		await vi.advanceTimersByTimeAsync(1000);
		await settle();

		const isMarked = () =>
			document
				.querySelector('li[data-permalink="/bicep/glue"] button[aria-label="mark downloaded"]')!
				.getAttribute('aria-pressed');

		expect(isMarked()).toBe('false');

		// SoundCloud's own flow: the row's More button, then "Download file" in
		// the menu it renders detached at the end of <body>.
		const row = [...document.querySelectorAll('.trackItem')].find((el) => el.textContent.includes('Glue'))!;
		row.querySelector('.sc-button-more')!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
		document.body.insertAdjacentHTML('beforeend', moreMenu());
		document
			.querySelector('.moreActions .sc-button-download')!
			.dispatchEvent(new MouseEvent('click', { bubbles: true }));

		await repaint();
		await settle();

		expect(isMarked()).toBe('true');
		expect(markers()).toEqual([{ kind: 'downloaded', row: 'Glue' }]);
	});

	it('shows a mark made before the page was opened, on both sides', async () => {
		await openWithDetails({ downloaded: { '/fourtet/baby': '2026-01-01T00:00:00.000Z' } });
		await repaint();
		expect(markers()).toEqual([{ kind: 'downloaded', row: 'Baby' }]);

		await mountPanel('manager');
		await addPage();
		await vi.advanceTimersByTimeAsync(1000);
		await settle();
		expect(
			document
				.querySelector('li[data-permalink="/fourtet/baby"] button[aria-label="mark downloaded"]')!
				.getAttribute('aria-pressed')
		).toBe('true');
	});
});

// ---------------------------------------------------------------------------

describe('the session queue between two tools', () => {
	it('carries a track picked in one panel to the panel that works through it', async () => {
		await openPage(
			playlistPage({
				rows: [playlistRow({ artist: 'Bicep', title: 'Glue', href: `/bicep/glue?in=curator/sets/my-playlist` })]
			})
		);
		hydrationScript([
			apiClientEntry(),
			{
				hydratable: 'playlist',
				data: {
					kind: 'playlist',
					permalink_url: `https://soundcloud.com${SET}`,
					tracks: [apiTrack({ id: 1, permalink: '/bicep/glue', downloadable: true })]
				}
			}
		]);
		mockFetch();
		disposeContentScripts();
		await loadContentScript();

		await mountPanel('manager');
		await addPage();
		await vi.advanceTimersByTimeAsync(1000);
		await settle();

		await click(
			document.querySelector('li[data-permalink="/bicep/glue"] button[aria-label="add to the session queue"]')
		);
		expect(chrome.testStore('session').queue.map((item) => item.permalink)).toEqual(['/bicep/glue']);

		// The popup opened again, as every click that opens a tab makes it: the
		// queue is in the header of whatever view comes back, and the sheet it
		// opens reads the same session storage.
		document.body.innerHTML = '';
		await mountApp();
		await openQueue();
		expect(rows().some((li) => li.textContent.includes('Glue'))).toBe(true);
	});
});

// ---------------------------------------------------------------------------

describe('the details cache shared between contexts', () => {
	// Two loads of the same page. The first learns the track's details from the
	// page's own hydration and writes them to storage; the second has no
	// hydration to learn them from, so an answer that costs no request can only
	// have come out of what the first one wrote.
	it('reuses what an earlier load of the page looked up rather than asking again', async () => {
		await openPage(
			playlistPage({
				rows: [playlistRow({ artist: 'Bicep', title: 'Glue', href: `/bicep/glue?in=curator/sets/my-playlist` })]
			})
		);

		const hydration = hydrationScript([
			apiClientEntry(),
			{
				hydratable: 'playlist',
				data: {
					kind: 'playlist',
					permalink_url: `https://soundcloud.com${SET}`,
					tracks: [apiTrack({ id: 1, permalink: '/bicep/glue', downloadable: true })]
				}
			}
		]);

		const { fn } = mockFetch();
		disposeContentScripts();
		await loadContentScript();

		await mountPanel('manager');
		await addPage();
		await vi.advanceTimersByTimeAsync(1000);
		await settle();
		expect(text()).toContain('soundcloud.com');

		// The page writes what it learned back a moment later.
		await vi.advanceTimersByTimeAsync(3000);
		expect(chrome.testStore().details).toHaveProperty('/bicep/glue');

		// Loaded again, with the API key and nothing else to answer from.
		hydration.remove();
		hydrationScript([apiClientEntry()]);
		disposeContentScripts();
		await loadContentScript();

		await mountPanel('manager');
		await addPage();
		await vi.advanceTimersByTimeAsync(1000);
		await settle();
		expect(text()).toMatch(/refreshed 1 track from My Playlist/);
		expect(text()).not.toMatch(/couldn’t check/);
		expect(fn).not.toHaveBeenCalled();
	});
});
