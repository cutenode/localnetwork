// content.ts — the queue button on cover art: where it turns up, what it
// says, and what a click on it writes into the session queue.
//
// It is also where the page says a track *is* in the queue — that used to be a
// pill of its own in the opposite corner — so its resting word carries the
// state ("queued") and its `data-hover` the word it takes under the pointer
// ("unqueue").
//
// Hovering itself is CSS (content.css keeps the button at opacity 0 until the
// artwork is hovered, the button is focused, or the track is in the queue, and
// swaps the resting word for `data-hover`), so what is asserted here is that
// the button is on the page, says the right thing in both words, and toggles
// the right track — the part a stylesheet cannot get wrong.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import { loadContentScript, disposeContentScripts } from '../helpers/load.ts';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';
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
	ART
} from '../helpers/dom.ts';
import type { KnownDetails } from '$lib/types';

declare const chrome: ChromeMock;

const NOW = new Date('2026-09-08T12:00:00Z').getTime();

beforeEach(() => {
	vi.useFakeTimers();
});

afterEach(disposeContentScripts);

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

// The artwork of the row that names a track, whichever layout it is in.
const artOf = (permalink: string) =>
	document
		.querySelector(`a[href^="${permalink}"]`)!
		.closest('.trackItem, .sound, .audibleTile')!
		.querySelector('.sc-artwork')!;

const buttonIn = (art: Element | null) => art!.querySelector<HTMLElement>(':scope > .tbx-queue');

const buttonOf = (permalink: string) => buttonIn(artOf(permalink))!;

// What a button says, as a reader and a screen reader would have it.
const readButton = (button: HTMLElement) => ({
	text: button.textContent,
	// What content.css puts in its place under the pointer, where there is one.
	hover: button.dataset.hover,
	label: button.getAttribute('aria-label'),
	title: button.title,
	className: button.className,
	state: button.dataset.state,
	permalink: button.dataset.permalink
});

const queueIn = () => chrome.testStore('session').queue;

// A click as the page delivers one: bubbling and cancelable, so the button's
// own preventDefault and stopPropagation can be seen to do something.
const clickQueue = async (button: Element | null) => {
	button!.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
	await flush();
};

describe('where the queue button turns up', () => {
	it('sits on every layout, in the variant that layout’s artwork takes', async () => {
		render(
			playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }) +
				publicLikesPage({ items: [likedSound({ href: '/l/one' })] }) +
				ownLikesPage({ items: [likedTile({ href: '/t/one' })] }),
			'/c/sets/p'
		);
		await loadContentScript();

		expect(readButton(buttonOf('/a/one'))).toMatchObject({
			text: '+',
			// Nothing to swap: a click adds either way, so only the shade changes.
			hover: undefined,
			label: 'add to the session queue',
			title: 'add to the session queue',
			className: 'tbx-queue tbx-queue--small',
			state: 'small:no',
			permalink: '/a/one'
		});
		expect(readButton(buttonOf('/l/one'))).toMatchObject({
			text: '+ queue',
			className: 'tbx-queue tbx-queue--large',
			state: 'large:no'
		});
		expect(readButton(buttonOf('/t/one'))).toMatchObject({
			text: '+ queue',
			state: 'large:no'
		});
	});

	it('sits on a track page’s hero and on the player bar', async () => {
		render(trackPage({ uploader: 'uploader' }) + playerBar({ href: '/artist/title' }), '/uploader/single-track');
		await loadContentScript();

		const hero = document.querySelector('.listenArtworkWrapper__artwork .sc-artwork');
		expect(readButton(buttonIn(hero)!)).toMatchObject({
			text: '+ queue',
			state: 'large:no',
			permalink: '/uploader/single-track'
		});

		const bar = document.querySelector('.playbackSoundBadge__avatar .sc-artwork');
		expect(readButton(buttonIn(bar)!)).toMatchObject({
			text: '+',
			state: 'small:no',
			permalink: '/artist/title'
		});
	});

	it('leaves alone a row that names no track, and a playlist page’s own hero', async () => {
		render(playlistPage({ rows: [playlistRow({ href: null })] }) + playerBar({ href: null }), '/c/sets/p');
		await loadContentScript();

		expect(buttonIn(document.querySelector('.trackItem__image .sc-artwork'))).toBe(null);
		expect(buttonIn(document.querySelector('.playbackSoundBadge__avatar .sc-artwork'))).toBe(null);
		// The hero of a playlist is not a track to queue, and renders the same
		// .fullHero a track page's does.
		expect(buttonIn(document.querySelector('.listenArtworkWrapper__artwork .sc-artwork'))).toBe(null);
	});

	it('takes the button off again when the row is recycled for no track', async () => {
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }), '/c/sets/p');
		await loadContentScript();
		const art = artOf('/a/one');
		expect(buttonIn(art)).not.toBe(null);
		expect(art.classList.contains('tbx-mark-host')).toBe(true);

		document.querySelector('.trackItem__trackTitle')!.removeAttribute('href');
		art.append(document.createElement('span')); // a render, as SoundCloud's are
		await vi.advanceTimersByTimeAsync(200);
		await flush();

		expect(buttonIn(art)).toBe(null);
		expect(art.classList.contains('tbx-mark-host')).toBe(false);
	});

	it('offers no button at all where session storage is shut to the page', async () => {
		globalThis.chrome = createChrome();
		chrome.storage.session.get.mockRejectedValue(new Error('Access to storage is not allowed from this context.'));
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }), '/c/sets/p');
		await loadContentScript();

		// A button there could not be written anywhere; the artwork carries
		// nothing, not even the positioning class.
		const art = artOf('/a/one');
		expect(buttonIn(art)).toBe(null);
		expect(art.classList.contains('tbx-mark-host')).toBe(false);
	});
});

describe('what a click on it does', () => {
	it('queues the track the way the panel would, artwork and details and all', async () => {
		globalThis.chrome = createChrome({
			storage: { details: { '/uploader/liked-track': details('/uploader/liked-track') } }
		});
		render(
			publicLikesPage({
				items: [
					likedSound({
						href: '/uploader/liked-track',
						title: 'Liked Track',
						displayArtist: 'BARELY ALIVE',
						artwork: ART('sound', 't500x500'),
						genre: 'Dubstep',
						liked: true
					})
				]
			}),
			'/dj/likes'
		);
		await loadContentScript();

		await clickQueue(buttonOf('/uploader/liked-track'));

		// Everything the row said, the three page-only facts included: a queued
		// track has no row to go back and read, and the Manager's list adds from
		// the queue.
		expect(queueIn()).toEqual([
			{
				permalink: '/uploader/liked-track',
				title: 'Liked Track',
				artist: 'BARELY ALIVE',
				artworkUrl: ART('sound', 't500x500'),
				details: details('/uploader/liked-track'),
				genre: 'Dubstep',
				liked: true,
				reposted: false
			}
		]);
	});

	// A row whose thumbnail never lazy-loaded shows no cover, but what is known
	// about the track names one — so it goes into the queue with a picture
	// rather than a placeholder. See detailsOf in content.js.
	it('takes the cover from the details when the row never loaded one', async () => {
		const cover = ART('from-the-api', 'large');
		globalThis.chrome = createChrome({
			storage: { details: { '/a/one': details('/a/one', { artworkUrl: cover }) } }
		});
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p', artwork: '' })] }), '/c/sets/p');
		await loadContentScript();

		await clickQueue(buttonOf('/a/one'));

		expect(queueIn()[0]).toMatchObject({ permalink: '/a/one', artworkUrl: cover });
	});

	it('reads a playlist row, a tile, a hero and the player bar for their own fields', async () => {
		render(
			playlistPage({
				rows: [
					playlistRow({
						href: '/a/one?in=c/sets/p',
						artist: 'Row Artist',
						title: 'Row Title',
						artwork: ART('row')
					})
				]
			}) +
				ownLikesPage({
					items: [likedTile({ href: '/t/one', artist: 'Tile Artist', title: 'Tile Title', artwork: ART('tile') })]
				}),
			'/c/sets/p'
		);
		await loadContentScript();

		await clickQueue(buttonOf('/a/one'));
		await clickQueue(buttonOf('/t/one'));

		expect(queueIn()).toEqual([
			{
				permalink: '/a/one',
				title: 'Row Title',
				artist: 'Row Artist',
				artworkUrl: ART('row'),
				details: null,
				// A playlist row renders no tag, and its buttons are off.
				genre: null,
				liked: false,
				reposted: false
			},
			{
				permalink: '/t/one',
				// The tile's heading nests an icon span, which is not part of the title.
				title: 'Tile Title',
				artist: 'Tile Artist',
				artworkUrl: ART('tile'),
				details: null,
				// A tile renders none of the three, so it says none of them.
				genre: null,
				liked: null,
				reposted: null
			}
		]);
	});

	// A track page's like and repost buttons are in the engagement bar below
	// the hero, not in it. A page rendering no such bar has nowhere to read
	// them from, which is "nobody knows" rather than "not liked".
	it('says nothing about the like and repost where a track page has no engagement bar', async () => {
		render(trackPage({ engagement: false, genre: 'Techno' }), '/uploader/single-track');
		await loadContentScript();

		await clickQueue(buttonIn(document.querySelector('.listenArtworkWrapper__artwork .sc-artwork')));

		expect(queueIn()[0]).toMatchObject({ genre: 'Techno', liked: null, reposted: null });
	});

	it('reads the hero and the player bar, which name their artist differently', async () => {
		render(
			trackPage({
				uploader: 'uploader',
				uploaderName: 'Uploader',
				title: 'Single Track',
				artwork: ART('hero'),
				genre: 'Techno',
				reposted: true
			}) +
				playerBar({ href: '/artist/title', title: 'Playing Track', artwork: ART('player'), artist: 'Playing Artist' }),
			'/uploader/single-track'
		);
		await loadContentScript();

		await clickQueue(buttonIn(document.querySelector('.listenArtworkWrapper__artwork .sc-artwork')));
		await clickQueue(buttonIn(document.querySelector('.playbackSoundBadge__avatar .sc-artwork')));

		expect(queueIn()).toEqual([
			{
				permalink: '/uploader/single-track',
				title: 'Single Track',
				artist: 'Uploader',
				artworkUrl: ART('hero'),
				details: null,
				genre: 'Techno',
				liked: false,
				reposted: true
			},
			{
				permalink: '/artist/title',
				title: 'Playing Track',
				// The uploader link above the title, rather than one inside it.
				artist: 'Playing Artist',
				artworkUrl: ART('player'),
				details: null,
				// The bar carries no tag and neither button, so it says nothing about
				// the three.
				genre: null,
				liked: null,
				reposted: null
			}
		]);

		// The next track, still loading: the bar names no uploader until it has.
		render(playerBar({ href: '/artist/loading' }), '/uploader/single-track');
		await vi.advanceTimersByTimeAsync(200);
		await clickQueue(buttonIn(document.querySelector('.playbackSoundBadge__avatar .sc-artwork')));
		expect(queueIn()[2]).toMatchObject({ permalink: '/artist/loading', artist: '' });
	});

	it('says queued, offers unqueue under the pointer, and puts the track back out', async () => {
		render(
			playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }) +
				publicLikesPage({ items: [likedSound({ href: '/l/one' })] }),
			'/c/sets/p'
		);
		await loadContentScript();

		await clickQueue(buttonOf('/a/one'));
		await clickQueue(buttonOf('/l/one'));
		// A 32 px thumbnail has room for a sign: the "+" the pill used to carry,
		// and the "−" hovering would make of it.
		expect(readButton(buttonOf('/a/one'))).toMatchObject({
			text: '+',
			hover: '−',
			label: 'take out of the session queue',
			state: 'small:yes'
		});
		expect(readButton(buttonOf('/l/one'))).toMatchObject({
			text: 'queued',
			hover: 'unqueue',
			label: 'take out of the session queue',
			state: 'large:yes'
		});
		// And nothing in the other corner: the button is the whole of what the
		// page says about the queue.
		expect(artOf('/a/one').querySelector('.tbx-mark')).toBe(null);

		await clickQueue(buttonOf('/a/one'));
		expect(queueIn().map((entry) => entry.permalink)).toEqual(['/l/one']);
		expect(readButton(buttonOf('/a/one'))).toMatchObject({
			text: '+',
			hover: undefined,
			state: 'small:no'
		});
	});

	it('says queued for what was queued before the page loaded, and adds behind it', async () => {
		globalThis.chrome = createChrome({
			session: {
				queue: [{ permalink: '/already/there', title: 'Already', artist: 'A', artworkUrl: '', details: null }]
			}
		});
		render(
			publicLikesPage({ items: [likedSound({ href: '/l/one' }), likedSound({ href: '/already/there' })] }),
			'/dj/likes'
		);
		await loadContentScript();

		// Queued already, from the popup: the button says so before anything is
		// clicked here.
		expect(readButton(buttonOf('/already/there'))).toMatchObject({ text: 'queued', state: 'large:yes' });
		expect(readButton(buttonOf('/l/one'))).toMatchObject({ text: '+ queue', state: 'large:no' });

		await clickQueue(buttonOf('/l/one'));

		expect(queueIn().map((entry) => entry.permalink)).toEqual(['/already/there', '/l/one']);
	});

	// The popup is where the queue is kept, and it is a different context: what
	// it writes arrives here as a change to the session area.
	it('follows the queue as the popup adds to it and takes things out', async () => {
		render(publicLikesPage({ items: [likedSound({ href: '/l/one' })] }), '/dj/likes');
		await loadContentScript();
		const state = () => buttonOf('/l/one').dataset.state;
		expect(state()).toBe('large:no');

		chrome.testEmitStorageChange({ queue: { newValue: [{ permalink: '/l/one' }] } }, 'session');
		expect(state()).toBe('large:yes');

		// Emptied, and an entry with no permalink is no entry at all.
		chrome.testEmitStorageChange({ queue: { newValue: [{ title: 'One' }] } }, 'session');
		expect(state()).toBe('large:no');

		// Nor is a queue that is not a list: the key cleared away, or written by
		// a version of this extension that kept something else under it.
		chrome.testEmitStorageChange({ queue: { newValue: [{ permalink: '/l/one' }] } }, 'session');
		chrome.testEmitStorageChange({ queue: { newValue: undefined } }, 'session');
		expect(state()).toBe('large:no');

		chrome.testEmitStorageChange({ queue: { newValue: [{ permalink: '/l/one' }] } }, 'session');
		// A local-area key of the same name is not the queue, and nothing else in
		// the session area is.
		chrome.testEmitStorageChange({ queue: { newValue: [] } });
		chrome.testEmitStorageChange({ view: { newValue: 'menu' } }, 'session');
		expect(state()).toBe('large:yes');
	});

	it('neither plays the track nor follows the link it sits in', async () => {
		render(ownLikesPage({ items: [likedTile({ href: '/t/one' })] }), '/you/likes');
		await loadContentScript();
		const onTile = vi.fn<(event: Event) => void>();
		document.querySelector('.audibleTile')!.addEventListener('click', onTile);

		const button = buttonOf('/t/one');
		const event = new MouseEvent('click', { bubbles: true, cancelable: true });
		button.dispatchEvent(event);
		await flush();

		expect(event.defaultPrevented).toBe(true);
		expect(onTile).not.toHaveBeenCalled();
		expect(queueIn().map((entry) => entry.permalink)).toEqual(['/t/one']);
	});

	it('queues the track the button was last painted for, not the one it was made for', async () => {
		render(playlistPage({ rows: [playlistRow({ href: '/a/one?in=c/sets/p' })] }), '/c/sets/p');
		await loadContentScript();
		const art = artOf('/a/one');

		// SoundCloud recycles the row for another track as the list scrolls.
		const link = document.querySelector('.trackItem__trackTitle')!;
		link.setAttribute('href', '/a/two?in=c/sets/p');
		link.textContent = 'Another Title';
		art.append(document.createElement('span'));
		await vi.advanceTimersByTimeAsync(200);
		await flush();

		await clickQueue(buttonIn(art));
		expect(queueIn()).toEqual([
			{
				permalink: '/a/two',
				title: 'Another Title',
				artist: 'Artist',
				artworkUrl: ART('row'),
				details: null,
				genre: null,
				liked: false,
				reposted: false
			}
		]);
	});

	it('leaves details out where what is known about the track has gone stale', async () => {
		const month = 31 * 24 * 60 * 60 * 1000;
		globalThis.chrome = createChrome({
			storage: { details: { '/l/one': details('/l/one', { at: NOW - 2 * month }) } }
		});
		render(publicLikesPage({ items: [likedSound({ href: '/l/one' })] }), '/dj/likes');
		await loadContentScript();

		await clickQueue(buttonOf('/l/one'));
		expect(queueIn()[0].details).toBe(null);
	});

	it('shrugs off a session storage that shuts under it', async () => {
		render(publicLikesPage({ items: [likedSound({ href: '/l/one' })] }), '/dj/likes');
		await loadContentScript();
		chrome.storage.session.set.mockRejectedValue(new Error('Extension context invalidated'));

		await clickQueue(buttonOf('/l/one'));

		expect(chrome.storage.session.set).toHaveBeenCalledTimes(1);
		expect(queueIn()).toBeUndefined();
		// The page still shows the queue it last read.
		expect(readButton(buttonOf('/l/one'))).toMatchObject({ state: 'large:no' });
	});
});
