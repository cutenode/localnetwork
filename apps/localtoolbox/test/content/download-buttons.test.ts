// content.ts — our download button in the place of SoundCloud's own.
//
// Two controls are replaced: "Download file" in a track's More menu, and the
// purchase link ("Buy", "Free Download", …) on Likes rows and track pages.
// What goes in their place is the button the Manager panel puts on the same
// track, doing what that one does — the track into the session queue, then
// the link opened or the file downloaded. It says the same word wherever it
// stands, behind a download icon, and leaves the one thing that differs — the
// domain the file would come from — to its tooltip.
//
// The setting behind it is chrome.storage.local's `settings.replaceDownloadButtons`,
// which the popup's settings sheet writes (src/lib/settings.svelte.ts) and
// this reads; test/consistency.test.ts holds the key and the default together.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import { loadContentScript, disposeContentScripts } from '../helpers/load.ts';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';
import { flush } from '../helpers/flush.ts';
import {
	render,
	playlistPage,
	playlistRow,
	publicLikesPage,
	likedSound,
	trackPage,
	v2TrackPage,
	moreMenu,
	ART,
	type LikedSoundOptions,
	type V2TrackPageOptions
} from '../helpers/dom.ts';
import type { KnownDetails } from '$lib/types';

declare const chrome: ChromeMock;

const NOW = new Date('2026-09-08T12:00:00Z').getTime();

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
	downloadable: true,
	hasDownloadsLeft: true,
	purchaseUrl: '',
	purchaseTitle: '',
	genre: '',
	artworkUrl: '',
	at: NOW,
	...extra
});

// Anchors would make jsdom try to navigate; our button never cancels its own
// click — the browser following the link is the whole point — so the test
// cancels it, after it has been seen.
const stopNavigation = () =>
	document.addEventListener('click', (event) => event.preventDefault(), {
		signal: listening.signal
	});

// The coalesced repaint: the MutationObserver's microtask, then the 150 ms
// the pass is bundled into.
const paint = async () => {
	await flush();
	vi.advanceTimersByTime(200);
	await flush();
};

// SoundCloud renders the More menu when its button is clicked, detached from
// the row it belongs to and appended to <body>. That insertion is what brings
// the content script round to it, so this does the two in that order.
const openMore = async (more: Element | null) => {
	if (!(more instanceof HTMLElement)) throw new Error('no More button on the row');
	more.click();
	document.body.insertAdjacentHTML('beforeend', moreMenu());
	await paint();
};

// Ours is an <a> where it opens a link and a <button> where it presses SoundCloud's.
const ours = (selector = '') => document.querySelector<HTMLAnchorElement | HTMLButtonElement>(`.tbx-dl${selector}`)!;

const allOurs = () => [...document.querySelectorAll<HTMLElement>('.tbx-dl')];

const nativeDownload = () => document.querySelector('.sc-button-download');

const hidden = (el: Element | null | undefined) => el!.classList.contains('tbx-dl-replaced');

const read = (el: HTMLElement) => ({
	tag: el.tagName,
	text: el.textContent,
	href: el.getAttribute('href'),
	title: el.title,
	label: el.getAttribute('aria-label'),
	permalink: el.dataset.permalink,
	className: el.className
});

// The icon is a picture of the word beside it, so it says nothing to a screen
// reader and nothing to `textContent`.
const iconIn = (el: Element) => el.querySelector('svg.tbx-dl__icon');

const queueIn = () => chrome.testStore('session').queue;

const downloadedStore = () => chrome.testStore().downloaded;

const click = async (el: Element | undefined) => {
	el!.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
	await flush();
};

// A Likes page with one row whose uploader set a purchase link.
const likesWithPurchase = (extra: LikedSoundOptions = {}) =>
	publicLikesPage({
		items: [
			likedSound({
				href: '/l/one',
				artist: 'Uploader',
				displayArtist: 'BARELY ALIVE',
				title: 'Liked Track',
				artwork: ART('sound', 't500x500'),
				purchase: { url: 'https://hypeddit.com/x', label: 'Free Download' },
				...extra
			})
		]
	});

describe('where our button goes', () => {
	it('stands in for a purchase link in a Likes row, with SoundCloud’s hidden', async () => {
		globalThis.chrome = createChrome();
		render(likesWithPurchase(), '/dj/likes');
		await loadContentScript();

		const native = document.querySelector('.soundActions__purchaseLink')!;
		expect(hidden(native)).toBe(true);
		expect(native.nextElementSibling).toBe(ours());
		expect(read(ours())).toMatchObject({
			tag: 'A',
			text: 'Download',
			// The gate.sc wrapper is unwrapped, exactly as the panel unwraps it.
			href: 'https://hypeddit.com/x',
			// Where from, which is what the word no longer says.
			title: 'hypeddit.com',
			label: 'download from hypeddit.com',
			permalink: '/l/one',
			className: 'tbx-dl tbx-dl--external'
		});
		expect(iconIn(ours())).not.toBe(null);
	});

	it('stands in for the purchase link on a track page', async () => {
		globalThis.chrome = createChrome();
		render(
			trackPage({ uploader: 'uploader', purchase: { url: 'https://buy.test', label: 'Buy' } }),
			'/uploader/single-track'
		);
		await loadContentScript();

		expect(hidden(document.querySelector('.sc-buylink'))).toBe(true);
		expect(read(ours())).toMatchObject({ tag: 'A', href: 'https://buy.test', permalink: '/uploader/single-track' });
	});

	it('stands in for Download file once the More menu it lives in is open', async () => {
		globalThis.chrome = createChrome();
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }), '/c/sets/p');
		await loadContentScript();

		await openMore(document.querySelector('.trackItem .sc-button-more'));
		expect(hidden(nativeDownload())).toBe(true);
		expect(read(ours())).toMatchObject({
			tag: 'BUTTON',
			text: 'Download',
			href: null,
			title: 'soundcloud.com',
			label: 'download from soundcloud.com',
			permalink: '/a/one',
			className: 'tbx-dl tbx-dl--soundcloud'
		});
		expect(ours().type).toBe('button');
		expect(iconIn(ours())).not.toBe(null);
	});

	it('leaves a Download file button alone while nothing names its track', async () => {
		globalThis.chrome = createChrome();
		// A row with no title link names nothing, so neither does its More menu.
		render(playlistPage({ rows: [playlistRow({ href: null })] }) + moreMenu(), '/c/sets/p');
		await loadContentScript();

		expect(hidden(nativeDownload())).toBe(false);
		expect(allOurs()).toEqual([]);
	});

	it('leaves a purchase link alone when it points nowhere', async () => {
		globalThis.chrome = createChrome();
		render(likesWithPurchase(), '/dj/likes');
		document.querySelector('.soundActions__purchaseLink')!.removeAttribute('href');
		await loadContentScript();

		expect(hidden(document.querySelector('.soundActions__purchaseLink'))).toBe(false);
		expect(allOurs()).toEqual([]);
	});

	it('paints once and then only keeps up: a second pass makes no second button', async () => {
		globalThis.chrome = createChrome();
		render(likesWithPurchase(), '/dj/likes');
		await loadContentScript();
		const first = ours();

		document.querySelector('.soundList')!.insertAdjacentHTML('beforeend', '<li></li>');
		await paint();
		expect(allOurs()).toHaveLength(1);
		expect(ours()).toBe(first);
	});
});

describe('what our button says', () => {
	// The same word wherever it stands, whatever the page or the lookup calls
	// the link: it is the same click on every one of them.
	it('says Download, whatever SoundCloud and the lookup call the link', async () => {
		globalThis.chrome = createChrome({
			storage: {
				details: {
					'/l/one': details('/l/one', {
						purchaseUrl: 'https://hypeddit.com/x',
						purchaseTitle: 'Buy on Bandcamp'
					})
				}
			}
		});
		render(likesWithPurchase(), '/dj/likes');
		await loadContentScript();
		expect(ours().textContent).toBe('Download');

		globalThis.chrome = createChrome();
		render(
			trackPage({ uploader: 'uploader', purchase: { url: 'https://buy.test', label: 'Buy' } }),
			'/uploader/single-track'
		);
		await loadContentScript();
		expect(ours().textContent).toBe('Download');
	});

	it('names the domain the file would come from, without its www', async () => {
		globalThis.chrome = createChrome();
		render(likesWithPurchase({ purchase: { url: 'https://www.beatport.com/track/1', label: 'Buy' } }), '/dj/likes');
		await loadContentScript();
		expect(ours().title).toBe('beatport.com');
	});

	// A gate.sc wrapper carries its destination in a query parameter, and there
	// is nothing to stop that being something which is no URL at all. Then the
	// destination itself is the most that can be said about where the click goes.
	it('falls back to the destination itself where it names no domain', async () => {
		globalThis.chrome = createChrome();
		render(likesWithPurchase(), '/dj/likes');
		document.querySelector('.soundActions__purchaseLink')!.setAttribute('href', 'https://gate.sc?url=not%20a%20url');
		await loadContentScript();
		expect(ours().title).toBe('not a url');
	});

	it('says when SoundCloud’s download limit is used up, as the panel does', async () => {
		globalThis.chrome = createChrome({
			storage: { details: { '/a/one': details('/a/one', { hasDownloadsLeft: false }) } }
		});
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }), '/c/sets/p');
		await loadContentScript();
		await openMore(document.querySelector('.trackItem .sc-button-more'));

		expect(ours().textContent).toBe('Download');
		expect(ours().title).toBe('soundcloud.com (no downloads left)');
	});

	// The tooltip is drawn before SoundCloud has been asked anything, and
	// brought up to date when the answer lands — from this tab or from another.
	it('takes up a spent download limit that is learned after the page was painted', async () => {
		globalThis.chrome = createChrome();
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }), '/c/sets/p');
		await loadContentScript();
		await openMore(document.querySelector('.trackItem .sc-button-more'));
		expect(ours().title).toBe('soundcloud.com');

		await chrome.storage.local.set({
			details: { '/a/one': details('/a/one', { hasDownloadsLeft: false }) }
		});
		await paint();
		expect(ours().title).toBe('soundcloud.com (no downloads left)');
		expect(allOurs()).toHaveLength(1);
	});
});

// SoundCloud sizes its own buttons, and differently from one layout to the
// next, so ours takes its height from the page rather than from a number
// written down here — and from the buttons standing *beside* it, which is what
// it has to line up with. jsdom lays nothing out (every offsetHeight is 0), so
// the heights are staged on the elements themselves.
describe('how tall our button is', () => {
	const setHeight = (el: Element, height: number) =>
		Object.defineProperty(el, 'offsetHeight', { value: height, configurable: true });

	// A Likes row's actions: More, the engagement buttons, then the purchase
	// link. The neighbours are what ours matches.
	const rowControls = () => document.querySelectorAll('.soundActions [class*="sc-button"]');

	it('is drawn as tall as the other buttons in the row', async () => {
		globalThis.chrome = createChrome();
		render(likesWithPurchase(), '/dj/likes');

		for (const el of rowControls()) setHeight(el, 28);
		await loadContentScript();

		expect(ours().style.height).toBe('28px');
	});

	// The point of measuring the neighbours rather than the control being
	// replaced: SoundCloud does not always size a purchase link like the buttons
	// it sits among, and it is the buttons ours has to line up with.
	it('matches the neighbours, not the control it stands in for', async () => {
		globalThis.chrome = createChrome();
		render(likesWithPurchase(), '/dj/likes');

		for (const el of rowControls()) setHeight(el, 28);
		setHeight(document.querySelector('.soundActions__purchaseLink')!, 21);
		await loadContentScript();

		expect(ours().style.height).toBe('28px');
	});

	// A More menu puts each item in a row of its own; the buttons to match are
	// the other items, which can be a level further out than a sibling.
	it('matches the other items of a More menu, through their wrappers', async () => {
		globalThis.chrome = createChrome();
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }), '/c/sets/p');
		await loadContentScript();

		const more = document.querySelector<HTMLElement>('.trackItem .sc-button-more')!;
		more.click();
		document.body.insertAdjacentHTML(
			'beforeend',
			`<div class="moreActions"><ul>
        <li><button class="sc-button-share" type="button">Share</button></li>
        <li><button class="sc-button-download" type="button"><span class="label">Download file</span></button></li>
      </ul></div>`
		);
		setHeight(document.querySelector('.moreActions .sc-button-share')!, 34);
		await paint();

		expect(ours('--soundcloud').style.height).toBe('34px');
	});

	// Measuring costs a layout, and a Likes page has hundreds of rows: one
	// control of a kind is measured and the rest are drawn to its answer.
	it('measures one row of a kind and draws the rest to match', async () => {
		globalThis.chrome = createChrome();
		render(
			publicLikesPage({
				items: [
					likedSound({ href: '/l/one', purchase: { url: 'https://hypeddit.com/x', label: 'Free Download' } }),
					likedSound({ href: '/l/two', purchase: { url: 'https://hypeddit.com/y', label: 'Free Download' } })
				]
			}),
			'/dj/likes'
		);
		const [first, second] = document.querySelectorAll('.soundActions');

		for (const el of first.querySelectorAll('[class*="sc-button"]')) setHeight(el, 28);

		for (const el of second.querySelectorAll('[class*="sc-button"]')) setHeight(el, 99); // never asked
		await loadContentScript();

		expect(allOurs().map((el) => el.style.height)).toEqual(['28px', '28px']);
	});

	// A control standing on its own, with no other button in the row: the one
	// being replaced is all there is to go by, and it is readable on the pass
	// that hides it.
	it('falls back to the control it replaces where the row holds nothing else', async () => {
		globalThis.chrome = createChrome();
		render(
			`<div class="sound">
        <div class="soundTitle"><a class="soundTitle__title" href="/p/one">One</a></div>
        <div class="soundActions">
          <a class="soundActions__purchaseLink sc-buylink" href="https://buy.test" aria-label="Buy">Buy</a>
        </div>
      </div>`,
			'/someone'
		);
		setHeight(document.querySelector('.soundActions__purchaseLink')!, 26);
		await loadContentScript();

		expect(ours().style.height).toBe('26px');
	});

	// Nothing laid out is no answer — a menu still opening, a row not yet
	// measured. The stylesheet's own padding stands, and the next pass asks
	// again rather than holding the button to a zero for good.
	it('leaves the height to the stylesheet until there is something to measure', async () => {
		globalThis.chrome = createChrome();
		render(likesWithPurchase(), '/dj/likes');
		await loadContentScript();
		expect(ours().style.height).toBe('');

		for (const el of rowControls()) setHeight(el, 28);
		document.querySelector('.soundList')!.insertAdjacentHTML('beforeend', '<li></li>');
		await paint();
		expect(ours().style.height).toBe('28px');
	});
});

describe('what a click on it does', () => {
	it('queues the track and lets the browser follow the link', async () => {
		globalThis.chrome = createChrome();
		render(likesWithPurchase(), '/dj/likes');
		await loadContentScript();
		stopNavigation();
		// Whether the click is still the browser's to follow once ours has had
		// it — asked on the way up, before stopNavigation takes it at the top.
		const followed: boolean[] = [];
		ours().parentElement!.addEventListener('click', (event) => followed.push(!event.defaultPrevented));

		await click(ours());
		expect(followed).toEqual([true]);
		expect(queueIn()).toEqual([
			{
				permalink: '/l/one',
				title: 'Liked Track',
				artist: 'BARELY ALIVE',
				artworkUrl: ART('sound', 't500x500'),
				details: null,
				genre: null,
				liked: false,
				reposted: false
			}
		]);
		// Opening a gate says nothing about having the file yet, so — exactly as
		// in the panel — nothing is marked.
		expect(downloadedStore()).toBeUndefined();
	});

	it('presses SoundCloud’s own Download file, which is what marks the track', async () => {
		globalThis.chrome = createChrome();
		render(
			playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p', artist: 'Bicep', title: 'Glue' })] }),
			'/c/sets/p'
		);
		await loadContentScript();
		await openMore(document.querySelector('.trackItem .sc-button-more'));

		await click(ours());
		// Marked by the click detection that has always watched that button —
		// its own section above — which is why nothing here writes a mark.
		expect(Object.keys(downloadedStore())).toEqual(['/a/one']);
		expect(queueIn()).toEqual([
			{
				permalink: '/a/one',
				title: 'Glue',
				artist: 'Bicep',
				artworkUrl: ART('row'),
				details: null,
				genre: null,
				liked: false,
				reposted: false
			}
		]);
	});

	it('carries the details it has, so the queue knows where the gate is', async () => {
		globalThis.chrome = createChrome({
			storage: { details: { '/l/one': details('/l/one', { purchaseUrl: 'https://hypeddit.com/x' }) } }
		});
		render(likesWithPurchase(), '/dj/likes');
		await loadContentScript();
		stopNavigation();

		await click(ours());
		expect(queueIn()[0].details).toMatchObject({ permalink: '/l/one', purchaseUrl: 'https://hypeddit.com/x' });
	});

	it('queues a track once however often it is clicked', async () => {
		globalThis.chrome = createChrome();
		render(likesWithPurchase(), '/dj/likes');
		await loadContentScript();
		stopNavigation();

		await click(ours());
		await click(ours());
		expect(queueIn()).toHaveLength(1);
	});

	it('names the track from the page’s hero where there is no row to read', async () => {
		globalThis.chrome = createChrome();
		render(
			trackPage({
				uploader: 'uploader',
				uploaderName: 'Uploader',
				title: 'Single Track',
				purchase: { url: 'https://buy.test', label: 'Buy' },
				genre: 'Techno',
				liked: true
			}),
			'/uploader/single-track'
		);
		await loadContentScript();
		stopNavigation();

		await click(ours());
		// The hero names the genre; the like and repost buttons are in the
		// engagement bar below it, which is where they are read from.
		expect(queueIn()).toEqual([
			{
				permalink: '/uploader/single-track',
				title: 'Single Track',
				artist: 'Uploader',
				artworkUrl: ART('hero', 't500x500'),
				details: null,
				genre: 'Techno',
				liked: true,
				reposted: false
			}
		]);
	});

	it('queues what little it has when the page has no hero either', async () => {
		globalThis.chrome = createChrome();
		render(trackPage({ hero: false, purchase: { url: 'https://buy.test', label: 'Buy' } }), '/uploader/single-track');
		await loadContentScript();
		stopNavigation();

		await click(ours());
		expect(queueIn()).toEqual([
			{ permalink: '/uploader/single-track', artworkUrl: '', details: null, genre: null, liked: null, reposted: null }
		]);
	});

	// Rows that no layout selector claims — a profile page's, say — still name
	// their track, and that is enough to queue one; the rest of the fields are
	// what the layout would have given and there is no layout to ask.
	it('queues the bare permalink from a row no layout claims', async () => {
		globalThis.chrome = createChrome();
		render(
			`<div class="sound">
        <div class="soundTitle"><a class="soundTitle__title" href="/p/one">One</a></div>
        <div class="soundActions">
          <a class="soundActions__purchaseLink sc-buylink" href="https://buy.test" aria-label="Buy">Buy</a>
        </div>
      </div>`,
			'/someone'
		);
		await loadContentScript();
		stopNavigation();

		await click(ours());
		expect(queueIn()).toEqual([
			{ permalink: '/p/one', artworkUrl: '', details: null, genre: null, liked: null, reposted: null }
		]);
	});

	// The queue lives in session storage, which is shut to a content script
	// until the popup opens it. The download is the part that must happen
	// anyway; the queue is what misses out.
	it('still downloads when the queue cannot be written', async () => {
		globalThis.chrome = createChrome();
		chrome.storage.session.get.mockRejectedValue(new Error('access denied'));
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }), '/c/sets/p');
		await loadContentScript();
		await openMore(document.querySelector('.trackItem .sc-button-more'));

		await click(ours());
		expect(Object.keys(downloadedStore())).toEqual(['/a/one']);
		expect(chrome.storage.session.set).not.toHaveBeenCalled();
	});
});

describe('the setting behind it', () => {
	it('leaves the page alone when it is off', async () => {
		globalThis.chrome = createChrome({ storage: { settings: { replaceDownloadButtons: false } } });
		render(likesWithPurchase(), '/dj/likes');
		await loadContentScript();

		expect(allOurs()).toEqual([]);
		expect(hidden(document.querySelector('.soundActions__purchaseLink'))).toBe(false);
	});

	it('puts SoundCloud’s own button back the moment it is turned off, and takes it again', async () => {
		globalThis.chrome = createChrome();
		render(likesWithPurchase(), '/dj/likes');
		await loadContentScript();
		expect(allOurs()).toHaveLength(1);

		await chrome.storage.local.set({ settings: { replaceDownloadButtons: false } });
		await flush();
		expect(allOurs()).toEqual([]);
		expect(hidden(document.querySelector('.soundActions__purchaseLink'))).toBe(false);

		await chrome.storage.local.set({ settings: { replaceDownloadButtons: true } });
		await flush();
		expect(allOurs()).toHaveLength(1);
		expect(hidden(document.querySelector('.soundActions__purchaseLink'))).toBe(true);
	});

	it.each([
		// Falsy, so a reader that took any value it was given would turn it off.
		['a value of the wrong type', { replaceDownloadButtons: 0 }],
		['a setting this version has never heard of', { somethingElse: 1 }],
		['something that is not an object at all', 'off']
	])('falls back to replacing, given %s', async (_what, settings) => {
		globalThis.chrome = createChrome({ storage: { settings } });
		render(likesWithPurchase(), '/dj/likes');
		await loadContentScript();
		expect(allOurs()).toHaveLength(1);
	});

	it('falls back to replacing when the setting cannot be read at all', async () => {
		globalThis.chrome = createChrome();
		const real = chrome.storage.local.get.getMockImplementation()!;
		chrome.storage.local.get.mockImplementation(async (keys) => {
			if (keys === 'settings') throw new Error('storage unavailable');

			return real(keys);
		});
		render(likesWithPurchase(), '/dj/likes');
		await loadContentScript();
		expect(allOurs()).toHaveLength(1);
	});
});

// ---- The signed-in track page ----------------------------------------------
//
// Which is a page inside a page: SoundCloud draws it in an iframe of its own
// at /n/<user>/<track>, as a React app with none of the classes the rest of
// this file matches on. The manifest's `all_frames` is what puts the content
// script in that frame; here it is simply the document under test, because
// jsdom has one document and the frame's is the only one that matters.

const V2_PURCHASE = { url: 'https://hypeddit.com/x', label: 'DOWNLOAD' };

const v2Page = (extra: V2TrackPageOptions = {}) =>
	v2TrackPage({
		uploader: 'y3llo',
		uploaderName: 'Y3llO',
		title: 'TAW GAWLI',
		artwork: ART('v2hero', 't500x500'),
		purchase: V2_PURCHASE,
		...extra
	});

const v2Native = () =>
	[...document.querySelectorAll('section[aria-label="Track header"] a[target="_blank"]')].find(
		(a) => !a.classList.contains('tbx-dl')
	);

describe('the signed-in layout’s cart', () => {
	// jsdom lays nothing out, so the row's heights are staged, exactly as they
	// are for the classic layouts above.
	const setHeight = (el: Element, height: number) =>
		Object.defineProperty(el, 'offsetHeight', { value: height, configurable: true });

	it('is replaced by our button, which says and does what every other one does', async () => {
		globalThis.chrome = createChrome();
		render(v2Page(), '/n/y3llo/taw-gawli');
		await loadContentScript();

		const native = v2Native()!;
		expect(hidden(native)).toBe(true);
		expect(read(ours())).toEqual({
			tag: 'A',
			text: 'Download',
			href: 'https://hypeddit.com/x',
			title: 'hypeddit.com',
			label: 'download from hypeddit.com',
			permalink: '/y3llo/taw-gawli',
			className: 'tbx-dl tbx-dl--v2'
		});
		expect(iconIn(ours())).toBeTruthy();
		// Ours stands in the row, beside the control it replaced.
		expect(native.nextElementSibling).toBe(ours());
	});

	it('queues the track the frame’s own header names, read out of that header', async () => {
		globalThis.chrome = createChrome();
		stopNavigation();
		render(v2Page(), '/n/y3llo/taw-gawli');
		await loadContentScript();

		await click(ours());
		expect(queueIn()).toEqual([
			expect.objectContaining({
				permalink: '/y3llo/taw-gawli',
				title: 'TAW GAWLI',
				artist: 'Y3llO',
				artworkUrl: ART('v2hero', 't500x500'),
				genre: 'Baile Funk',
				// Whether this account liked or reposted it is only ever the word on
				// a button here, so the layout says nothing rather than reading it.
				liked: null,
				reposted: null
			})
		]);
	});

	it('marks the track when SoundCloud’s own cart is clicked instead', async () => {
		globalThis.chrome = createChrome({ storage: { settings: { replaceDownloadButtons: false } } });
		stopNavigation();
		render(v2Page(), '/n/y3llo/taw-gawli');
		await loadContentScript();

		expect(allOurs()).toEqual([]);
		await click(v2Native());
		expect(Object.keys(downloadedStore())).toEqual(['/y3llo/taw-gawli']);
	});

	it('is as tall as the icon buttons it stands among', async () => {
		globalThis.chrome = createChrome();
		render(v2Page(), '/n/y3llo/taw-gawli');

		// The row's buttons are MUI's, so there is no sc-button class to find
		// them by; .MuiIconButton-root is the one class MUI does not hash.
		for (const button of document.querySelectorAll('.MuiIconButton-root')) {
			setHeight(button, 40);
		}

		await loadContentScript();

		expect(ours().style.height).toBe('40px');
	});

	it('replaces the cart in every header the page renders', async () => {
		globalThis.chrome = createChrome();
		// SoundCloud draws one header per breakpoint, each with its own cart;
		// only one is ever on screen, and both are replaced.
		render(v2Page({ headers: 2 }), '/n/y3llo/taw-gawli');
		await loadContentScript();

		expect(allOurs()).toHaveLength(2);
		expect(document.querySelectorAll('.tbx-dl-replaced')).toHaveLength(2);
	});

	it('leaves a header with no purchase link exactly as it was', async () => {
		globalThis.chrome = createChrome();
		render(v2Page({ purchase: null }), '/n/y3llo/taw-gawli');
		await loadContentScript();

		expect(allOurs()).toEqual([]);
	});

	it('names no track, and so replaces nothing, when the path and the header disagree', async () => {
		globalThis.chrome = createChrome();
		// The frame's path says one uploader and the header names another: the
		// same disagreement the classic page refuses to guess through.
		render(v2Page({ uploader: 'someone-else' }), '/n/y3llo/taw-gawli');
		await loadContentScript();

		expect(allOurs()).toEqual([]);
		expect(hidden(v2Native())).toBe(false);
	});

	it('still names the track from the path when the header names no uploader', async () => {
		globalThis.chrome = createChrome();
		stopNavigation();
		// Nothing left in the header but the genre tag and the purchase link, so
		// there is no uploader to check the path against and none to read: the
		// path is taken as it stands, as the classic hero takes it.
		render(v2Page({ uploaderLink: false }), '/n/y3llo/taw-gawli');
		await loadContentScript();

		expect(ours().dataset.permalink).toBe('/y3llo/taw-gawli');
		await click(ours());
		expect(queueIn()[0]).toMatchObject({ permalink: '/y3llo/taw-gawli', artist: '' });
	});

	// It is the tags index, and a genre under it, that is not an uploader — not
	// every account whose name happens to start with the same four letters.
	it('reads an uploader whose name starts with "tags" as the uploader', async () => {
		globalThis.chrome = createChrome();
		stopNavigation();
		render(v2Page({ uploader: 'tagsmusic', uploaderName: 'Tags Music' }), '/n/tagsmusic/taw-gawli');
		await loadContentScript();

		await click(ours());
		expect(queueIn()[0]).toMatchObject({ permalink: '/tagsmusic/taw-gawli', artist: 'Tags Music' });
	});

	// Not every link in the header that stays on this site is a profile: one
	// that goes deeper than a single segment is a track, a set, a page of a
	// profile — and it is not the uploader for coming before the one that is.
	it('does not take a link deeper than a profile for the uploader', async () => {
		globalThis.chrome = createChrome();
		stopNavigation();
		render(v2Page(), '/n/y3llo/taw-gawli');
		document
			.querySelector('section[aria-label="Track header"] a.MuiLink-root')!
			.insertAdjacentHTML(
				'beforebegin',
				'<a class="MuiLink-root" href="https://soundcloud.com/y3llo/popular-tracks"><span>Popular tracks</span></a>'
			);
		await loadContentScript();

		await click(ours());
		expect(queueIn()[0]).toMatchObject({ permalink: '/y3llo/taw-gawli', artist: 'Y3llO' });
	});

	it('replaces nothing where the frame is not showing a track', async () => {
		globalThis.chrome = createChrome();
		// The header is up but the path is not a track's: no track to name the
		// button with, so SoundCloud's own control is left exactly as it was.
		render(v2Page(), '/n/y3llo');
		await loadContentScript();

		expect(allOurs()).toEqual([]);
		expect(hidden(v2Native())).toBe(false);
	});

	it('reads a header that is missing the parts it can do without', async () => {
		globalThis.chrome = createChrome();
		stopNavigation();
		// No `title` on the heading (so the text it shows is the whole of it), no
		// cover art, and no genre tag — which is "the page never said", not "no
		// genre", exactly as a playlist row with no tag reports it.
		render(v2Page({ titleAttr: false, artwork: null, genre: null }), '/n/y3llo/taw-gawli');
		await loadContentScript();

		await click(ours());
		expect(queueIn()[0]).toMatchObject({
			title: 'TAW GAWLI',
			artworkUrl: '',
			genre: null
		});
	});

	it('obeys the setting, in the frame as on the page', async () => {
		globalThis.chrome = createChrome({ storage: { settings: { replaceDownloadButtons: false } } });
		render(v2Page(), '/n/y3llo/taw-gawli');
		await loadContentScript();
		expect(allOurs()).toEqual([]);

		await chrome.storage.local.set({ settings: { replaceDownloadButtons: true } });
		await flush();
		expect(allOurs()).toHaveLength(1);
	});
});
