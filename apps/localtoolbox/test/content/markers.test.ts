// content.ts — the markers painted on the page (Downloaded / nodl), how they
// follow storage and SoundCloud's lazy rendering, and the clicks on
// SoundCloud's own download buttons that create marks.
//
// Being in the session queue is not one of them: the queue button says that
// itself, in the opposite corner — see queue-button.test.ts.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import { loadContentScript, disposeContentScripts } from '../helpers/load.ts';
import { createChrome, type ChromeMock, type ChromeOptions } from '../helpers/chrome.ts';
import { flush } from '../helpers/flush.ts';
import {
	render,
	playlistPage,
	playlistRow,
	publicLikesPage,
	ownLikesPage,
	likedSound,
	likedTile,
	trackPage,
	playerBar,
	moreMenu
} from '../helpers/dom.ts';
import type { KnownDetails } from '$lib/types';

declare const chrome: ChromeMock;

const NOW = new Date('2026-09-08T12:00:00Z').getTime();

const ISO = '2026-09-08T12:00:00.000Z';

// The listeners a test puts on the document go when the test does, so no test
// clicks through another's.
let listening: AbortController;

beforeEach(() => {
	vi.useFakeTimers();
	listening = new AbortController();
});

afterEach(() => {
	listening.abort();
	disposeContentScripts();
});

const details = (permalink: string, extra: Partial<KnownDetails> = {}) => ({
	id: 1,
	permalink,
	downloadable: false,
	hasDownloadsLeft: true,
	purchaseUrl: '',
	purchaseTitle: '',
	genre: '',
	artworkUrl: '',
	at: NOW,
	...extra
});

// The markers inside an artwork element: [{ kind, pill }].
const markersIn = (art: Element) =>
	[...art.querySelectorAll<HTMLElement>(':scope > .tbx-mark')].map((m) => ({
		kind: m.dataset.kind,
		pill: m.querySelector('.tbx-mark__pill')!.textContent,
		className: m.className
	}));

const artOf = (permalink: string) =>
	document
		.querySelector(`a[href^="${permalink}"]`)!
		.closest('.trackItem, .sound, .audibleTile')!
		.querySelector('.sc-artwork')!;

const marksOf = (permalink: string) => markersIn(artOf(permalink));

// A page mixing all three row layouts, which the marker code treats alike.
const mixedPage = () =>
	playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' }), playlistRow({ href: '/a/two?in=c/sets/p' })] }) +
	publicLikesPage({ items: [likedSound({ href: '/l/one' })] }) +
	ownLikesPage({ items: [likedTile({ href: '/t/one' })] });

const downloadedStore = () => chrome.testStore().downloaded;

describe('painting marks', () => {
	it('paints a Downloaded mark on every layout, compact on playlist rows', async () => {
		globalThis.chrome = createChrome({ storage: { downloaded: { '/a/one': ISO, '/l/one': ISO, '/t/one': ISO } } });
		render(mixedPage(), '/c/sets/p');
		await loadContentScript();
		expect(marksOf('/a/one')).toEqual([
			{ kind: 'downloaded', pill: '✓', className: 'tbx-mark tbx-mark--downloaded tbx-mark--small' }
		]);
		expect(marksOf('/a/two')).toEqual([]);
		expect(marksOf('/l/one')).toEqual([
			{ kind: 'downloaded', pill: 'downloaded', className: 'tbx-mark tbx-mark--downloaded tbx-mark--large' }
		]);
		expect(marksOf('/t/one')).toEqual([
			{ kind: 'downloaded', pill: 'downloaded', className: 'tbx-mark tbx-mark--downloaded tbx-mark--large' }
		]);
	});

	it('paints nodl on tracks known to have no download link, unless they are downloaded', async () => {
		globalThis.chrome = createChrome({
			storage: {
				downloaded: { '/a/both': ISO },
				details: {
					'/a/nodl': details('/a/nodl'),
					'/a/both': details('/a/both'),
					'/a/buy': details('/a/buy', { purchaseUrl: 'https://buy.test' }),
					'/a/dl': details('/a/dl', { downloadable: true }),
					'/a/old': details('/a/old', { at: NOW - 40 * 24 * 60 * 60 * 1000 }),
					'/a/failed': { unavailable: true, at: NOW }
				}
			}
		});
		render(
			playlistPage({
				rows: ['nodl', 'both', 'buy', 'dl', 'old', 'failed'].map((n) => playlistRow({ href: `/a/${n}?in=c/sets/p` }))
			}),
			'/c/sets/p'
		);
		await loadContentScript();
		expect(marksOf('/a/nodl')).toEqual([
			{ kind: 'nodl', pill: 'nodl', className: 'tbx-mark tbx-mark--nodl tbx-mark--small' }
		]);
		expect(marksOf('/a/both').map((m) => m.kind)).toEqual(['downloaded']);

		for (const n of ['buy', 'dl', 'old', 'failed']) expect(marksOf(`/a/${n}`)).toEqual([]);
	});

	// The queue used to be a pill of its own here, in the corner opposite the
	// button that fills it. It is the button that says so now, so a queued track
	// that is nothing else carries no pill at all.
	it('paints no pill for a track that is merely in the session queue', async () => {
		globalThis.chrome = createChrome({
			session: { queue: [{ permalink: '/a/one' }, { permalink: '/l/one' }, { permalink: '/t/one' }] }
		});
		render(mixedPage(), '/c/sets/p');
		await loadContentScript();

		for (const permalink of ['/a/one', '/a/two', '/l/one', '/t/one']) {
			expect(marksOf(permalink), permalink).toEqual([]);
		}

		// The queue was read — the buttons say so — so no pill is the page
		// choosing not to paint one, not the page not knowing.
		const queueState = (permalink: string) =>
			artOf(permalink).querySelector<HTMLElement>(':scope > .tbx-queue')!.dataset.state;

		expect(['/a/one', '/a/two', '/l/one', '/t/one'].map(queueState)).toEqual([
			'small:yes',
			'small:no',
			'large:yes',
			'large:yes'
		]);
	});

	// A track page's own hero artwork carries the same pills as a row.
	// Nothing in the hero names the track, so the mark follows the page's own
	// identity (detectPage), and the hero nests one .sc-artwork inside another:
	// the outer box is the one marked, once.
	it("paints every pill on a track page's own hero artwork", async () => {
		for (const [store, kind, pill] of [
			[{ storage: { downloaded: { '/uploader/single-track': ISO } } }, 'downloaded', 'downloaded'],
			[{ storage: { details: { '/uploader/single-track': details('/uploader/single-track') } } }, 'nodl', 'nodl']
		] satisfies [ChromeOptions, string, string][]) {
			globalThis.chrome = createChrome(store);
			render(trackPage(), '/uploader/single-track');
			await loadContentScript();
			const art = document.querySelector('.listenArtworkWrapper__artwork > .sc-artwork')!;
			expect(markersIn(art)).toEqual([{ kind, pill, className: `tbx-mark tbx-mark--${kind} tbx-mark--large` }]);
			// The nested inner artwork is not a second place to paint.
			expect(document.querySelectorAll('.tbx-mark')).toHaveLength(1);
			disposeContentScripts();
		}
	});

	// The same .fullHero renders on a playlist page, where the artwork is the
	// playlist's own. A playlist is not a track to mark — even when a track of
	// the same path has been downloaded. (The rows name the playlist, so its
	// path is the one the page would give the hero if it gave it anything.)
	it("leaves a playlist's hero artwork alone", async () => {
		globalThis.chrome = createChrome({
			storage: { downloaded: { '/curator/sets/my-playlist': ISO, '/a/one': ISO } }
		});
		render(
			playlistPage({ rows: [playlistRow({ href: '/a/one?in=curator/sets/my-playlist' })] }),
			'/curator/sets/my-playlist'
		);
		await loadContentScript();
		expect(markersIn(document.querySelector('.listenArtworkWrapper__artwork > .sc-artwork')!)).toEqual([]);
		expect(marksOf('/a/one').map((m) => m.kind)).toEqual(['downloaded']);
	});

	// The player bar says what is playing wherever the page has scrolled to. Its
	// 30 px artwork takes the compact variant, and an ad — whose title is a bare
	// <span> naming no track — takes nothing.
	it('paints the player bar, compact, and leaves an ad alone', async () => {
		globalThis.chrome = createChrome({
			storage: { downloaded: { '/a/one': ISO }, details: { '/a/two': details('/a/two') } }
		});
		render(playerBar({ href: '/a/one' }), '/discover');
		await loadContentScript();
		const badgeArt = () => document.querySelector('.playbackSoundBadge__avatar .sc-artwork')!;
		expect(markersIn(badgeArt())).toEqual([
			{ kind: 'downloaded', pill: '✓', className: 'tbx-mark tbx-mark--downloaded tbx-mark--small' }
		]);

		// The bar is re-rendered as the next track starts: the mark follows it.
		render(playerBar({ href: '/a/two' }), '/discover');
		await vi.advanceTimersByTimeAsync(200);
		expect(markersIn(badgeArt())).toEqual([
			{ kind: 'nodl', pill: 'nodl', className: 'tbx-mark tbx-mark--nodl tbx-mark--small' }
		]);

		render(playerBar({ href: null }), '/discover');
		await vi.advanceTimersByTimeAsync(200);
		expect(document.querySelectorAll('.tbx-mark')).toHaveLength(0);
	});

	// Chrome shuts the session area to content scripts until the popup opens it,
	// and it stays shut in a browser too old to be asked: everything else on the
	// page carries on.
	it('paints the rest of the marks when the session area cannot be read', async () => {
		globalThis.chrome = createChrome({ storage: { downloaded: { '/a/one': ISO } } });
		chrome.storage.session.get.mockRejectedValue(new Error('Access to storage is not allowed from this context.'));
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }), '/c/sets/p');
		await loadContentScript();
		expect(marksOf('/a/one').map((m) => m.kind)).toEqual(['downloaded']);
		// With no queue button on it, the mark is the only thing that makes the
		// artwork the overlay's containing block — and it still has to be one.
		expect(artOf('/a/one').classList.contains('tbx-mark-host')).toBe(true);
	});

	it('skips rows without artwork or without a track link', async () => {
		globalThis.chrome = createChrome({ storage: { downloaded: { '/a/one': ISO, '/artist/title': ISO } } });
		render(
			playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' }), playlistRow({ href: null })] }),
			'/c/sets/p'
		);
		document.querySelector('.trackItem .sc-artwork')!.remove();
		await loadContentScript();
		expect(document.querySelectorAll('.tbx-mark')).toHaveLength(0);
	});

	it('follows the marks and details as storage changes, swapping and removing pills', async () => {
		globalThis.chrome = createChrome({ storage: { downloaded: { '/a/one': ISO } } });
		render(
			playlistPage({
				rows: [playlistRow({ href: '/a/one?in=c/sets/p' }), playlistRow({ href: '/a/two?in=c/sets/p' })]
			}),
			'/c/sets/p'
		);
		await loadContentScript();
		expect(marksOf('/a/one').map((m) => m.kind)).toEqual(['downloaded']);

		chrome.testEmitStorageChange({ downloaded: { newValue: undefined } });
		expect(marksOf('/a/one')).toEqual([]);

		chrome.testEmitStorageChange({ details: { newValue: { '/a/two': details('/a/two') } } });
		expect(marksOf('/a/two').map((m) => m.kind)).toEqual(['nodl']);

		chrome.testEmitStorageChange({ downloaded: { newValue: { '/a/two': ISO } } });
		expect(marksOf('/a/two').map((m) => m.kind)).toEqual(['downloaded']);

		// Changes in another storage area, or to other keys, are ignored.
		chrome.testEmitStorageChange({ downloaded: { newValue: {} } }, 'sync');
		chrome.testEmitStorageChange({ unrelated: { newValue: 1 } });
		expect(marksOf('/a/two').map((m) => m.kind)).toEqual(['downloaded']);
	});

	it('paints rows SoundCloud renders later, in one pass per burst', async () => {
		globalThis.chrome = createChrome({
			storage: { downloaded: { '/a/late': ISO, '/a/later': ISO, '/a/recycled': ISO } }
		});
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }), '/c/sets/p');
		await loadContentScript();
		const list = document.querySelector('.trackList__list')!;

		// The row on the page turns into a downloaded track without an element
		// being added — an attribute change, which never asks for a pass — so any
		// pass that runs from here on shows up as a mark on it.
		document.querySelector('.trackItem__trackTitle')!.setAttribute('href', '/a/recycled?in=c/sets/p');

		// A text-only mutation never brings a row: nothing to repaint.
		list.append(document.createTextNode('loading…'));
		await flush();
		await vi.advanceTimersByTimeAsync(150);
		expect(document.querySelectorAll('.tbx-mark')).toHaveLength(0);

		list.insertAdjacentHTML('beforeend', playlistRow({ href: '/a/late?in=c/sets/p' }));
		await flush();
		list.insertAdjacentHTML('beforeend', playlistRow({ href: '/a/later?in=c/sets/p' }));
		await flush();
		expect(document.querySelectorAll('.tbx-mark')).toHaveLength(0);
		await vi.advanceTimersByTimeAsync(150);
		expect(marksOf('/a/late').map((m) => m.kind)).toEqual(['downloaded']);
		expect(marksOf('/a/later').map((m) => m.kind)).toEqual(['downloaded']);
		expect(marksOf('/a/recycled').map((m) => m.kind)).toEqual(['downloaded']);
	});

	it('paints nothing, and does not watch the page, when storage is unavailable', async () => {
		chrome.storage.local.get.mockRejectedValue(new Error('Extension context invalidated'));
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }), '/c/sets/p');
		await loadContentScript();
		document
			.querySelector('.trackList__list')!
			.insertAdjacentHTML('beforeend', playlistRow({ href: '/a/two?in=c/sets/p' }));
		await flush();
		await vi.advanceTimersByTimeAsync(150);
		expect(document.querySelectorAll('.tbx-mark')).toHaveLength(0);
		// Nothing is downloaded that could be marked, but the session area did
		// answer: a pass over the new row would have put a queue button on it.
		expect(document.querySelectorAll('.tbx-queue')).toHaveLength(0);
		expect(chrome.storage.onChanged.addListener).not.toHaveBeenCalled();
	});
});

describe('noticing SoundCloud’s own download buttons', () => {
	// Anchors would make jsdom try to navigate; the content script never
	// cancels the click, so the test does, after it has been observed.
	const stopNavigation = () =>
		document.addEventListener('click', (event) => event.preventDefault(), {
			signal: listening.signal
		});

	it('marks the row whose More menu the Download file button came from', async () => {
		render(
			playlistPage({
				rows: [playlistRow({ href: '/a/one?in=c/sets/p' }), playlistRow({ href: '/a/two?in=c/sets/p' })]
			}) + moreMenu(),
			'/c/sets/p'
		);
		await loadContentScript();
		const [moreOne, moreTwo] = document.querySelectorAll<HTMLElement>('.trackItem .sc-button-more');

		moreTwo.click();
		document.querySelector<HTMLElement>('.sc-button-download .label')!.click();
		await flush();
		expect(downloadedStore()).toEqual({ '/a/two': ISO });
		expect(marksOf('/a/two').map((m) => m.kind)).toEqual(['downloaded']);
		expect(marksOf('/a/one')).toEqual([]);

		moreOne.click();
		document.querySelector<HTMLElement>('.sc-button-download')!.click();
		await flush();
		expect(Object.keys(downloadedStore()).sort()).toEqual(['/a/one', '/a/two']);
	});

	it('prefers the row a Download file button sits in over the last More menu', async () => {
		render(
			publicLikesPage({ items: [likedSound({ href: '/l/one' }), likedSound({ href: '/l/two' })] }) + moreMenu(),
			'/dj/likes'
		);
		await loadContentScript();
		const rows = document.querySelectorAll('.sound');
		rows[0].querySelector<HTMLElement>('.sc-button-more')!.click();
		rows[1]
			.querySelector('.soundActions')!
			.insertAdjacentHTML('beforeend', '<button class="sc-button-download">Download file</button>');
		rows[1].querySelector<HTMLElement>('.sc-button-download')!.click();
		await flush();
		expect(downloadedStore()).toEqual({ '/l/two': ISO });
	});

	it('marks nothing for a Download file button without any context', async () => {
		render(playlistPage({ rows: [playlistRow({ href: null })] }) + moreMenu(), '/c/sets/p');
		await loadContentScript();
		document.querySelector<HTMLElement>('.trackItem .sc-button-more')!.click(); // a row without a link
		document.querySelector<HTMLElement>('.sc-button-download')!.click();
		await flush();
		expect(downloadedStore()).toBeUndefined();
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});

	it('marks a track whose purchase link is clicked in a Likes row', async () => {
		render(
			publicLikesPage({
				items: [likedSound({ href: '/l/one', purchase: { url: 'https://hypeddit.com/x', label: 'Free Download' } })]
			}),
			'/dj/likes'
		);
		await loadContentScript();
		stopNavigation();
		document.querySelector<HTMLElement>('.soundActions__purchaseLink')!.click();
		await flush();
		expect(downloadedStore()).toEqual({ '/l/one': ISO });
		expect(marksOf('/l/one').map((m) => m.kind)).toEqual(['downloaded']);
	});

	it('marks the page’s own track on a track page', async () => {
		render(
			trackPage({ uploader: 'uploader', purchase: { url: 'https://buy.test', label: 'Buy' } }) + moreMenu(),
			'/uploader/single-track'
		);
		await loadContentScript();
		stopNavigation();

		document.querySelector<HTMLElement>('.listenEngagement .sc-button-more')!.click();
		document.querySelector<HTMLElement>('.sc-button-download')!.click();
		await flush();
		expect(downloadedStore()).toEqual({ '/uploader/single-track': ISO });

		chrome.storage.local.set.mockClear();
		document.querySelector<HTMLElement>('.fullHero .sc-button-more')!.click();
		document.querySelector<HTMLElement>('.sc-button-download')!.click();
		document.querySelector<HTMLElement>('.listenEngagement .sc-buylink')!.click();
		await flush();
		expect(chrome.storage.local.set).toHaveBeenCalledTimes(2);
	});

	it('accepts the URL’s track when the hero names no uploader', async () => {
		render(trackPage({ purchase: { url: 'https://buy.test', label: 'Buy' } }), '/someone/some-track');
		document.querySelector('.soundTitle__username')!.remove();
		await loadContentScript();
		stopNavigation();
		document.querySelector<HTMLElement>('.sc-buylink')!.click();
		await flush();
		expect(downloadedStore()).toEqual({ '/someone/some-track': ISO });
	});

	it.each([
		['the address bar names another uploader', '/someone-else/single-track'],
		['the address bar is not a track path', '/uploader'],
		['the address bar is a deeper path', '/uploader/single-track/recommended']
	])('marks nothing on a track page when %s', async (_name, path) => {
		render(trackPage({ uploader: 'uploader', purchase: { url: 'https://buy.test', label: 'Buy' } }) + moreMenu(), path);
		await loadContentScript();
		stopNavigation();
		document.querySelector<HTMLElement>('.listenEngagement .sc-button-more')!.click();
		document.querySelector<HTMLElement>('.sc-button-download')!.click();
		document.querySelector<HTMLElement>('.sc-buylink')!.click();
		await flush();
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});

	it('marks nothing for a purchase link outside the track page’s hero and action bar', async () => {
		render(
			trackPage({ uploader: 'uploader' }) +
				'<aside><a class="sc-buylink" href="https://gate.sc?url=https%3A%2F%2Fbuy.test">Buy</a></aside>',
			'/uploader/single-track'
		);
		await loadContentScript();
		stopNavigation();
		document.querySelector<HTMLElement>('aside .sc-buylink')!.click();
		await flush();
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});

	it('ignores clicks elsewhere, and clicks that have no element target', async () => {
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }), '/c/sets/p');
		await loadContentScript();
		document.querySelector('.trackItem__trackTitle')!.closest<HTMLElement>('.trackItem')!.click();
		document.querySelector<HTMLElement>('.fullHero__tracksSummary')!.click();
		document.dispatchEvent(new MouseEvent('click', { bubbles: true }));
		await flush();
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});

	it('shrugs off a storage that refuses the mark', async () => {
		render(
			publicLikesPage({
				items: [likedSound({ href: '/l/one', purchase: { url: 'https://hypeddit.com/x', label: 'Free Download' } })]
			}),
			'/dj/likes'
		);
		await loadContentScript();
		stopNavigation();
		chrome.storage.local.set.mockRejectedValue(new Error('Extension context invalidated'));
		document.querySelector<HTMLElement>('.soundActions__purchaseLink')!.click();
		await flush();
		expect(chrome.storage.local.set).toHaveBeenCalledTimes(1);
		expect(downloadedStore()).toBeUndefined();
	});
});
