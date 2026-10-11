// The content script against pages it was never shown.
//
// content.ts reads a site it does not control, and SoundCloud ships a new
// build most weeks. The example tests say what it reads out of the markup as
// it is today; these say what must stay true when that markup is not as it is
// today — renamed classes, half-rendered lists, a hero with no title, a
// hydration blob that is not JSON, markup truncated mid-tag by a bad response.
//
// The bar is deliberately low and absolute: *always answer*. A panel shows
// what the page sends back, so a throw here is a tool that stops working
// rather than a tool that says it found nothing.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import * as fc from 'fast-check';
import { loadContentScript, disposeContentScripts } from '../helpers/load.ts';
import { mockFetch } from '../helpers/api.ts';
import type { ChromeMock } from '../helpers/chrome.ts';
import {
	apiClientEntry,
	render,
	playlistPage,
	playlistRow,
	systemPlaylistPage,
	publicLikesPage,
	ownLikesPage,
	likedSound,
	likedTile,
	tilePlaceholder,
	trackPage,
	moreMenu,
	hydrationScript,
	ART
} from '../helpers/dom.ts';
import type { DetailedTrack, TracksAnswer } from '$lib/types';

declare const chrome: ChromeMock;

const RUNS = { numRuns: 60 };

afterEach(disposeContentScripts);

// One load, many pages: the script re-detects the page on every message and
// keeps observing the same <body>, exactly as it does across SPA navigation.
beforeEach(async () => {
	vi.useFakeTimers();
	await loadContentScript();
});

const extract = () => chrome.testSendMessage({ type: 'EXTRACT_TRACKS' });

// --- generators --------------------------------------------------------------

// Every code point, and — since those almost never land on one — the
// whitespace and punctuation a reader of titles and links has to handle.
const text = () =>
	fc.oneof(
		fc.string({ unit: 'binary', maxLength: 30 }),
		fc.string({ unit: fc.constantFrom(...Array.from(' \n\t aZ-/?#&<"'), '  '), maxLength: 30 })
	);

const rowArb = () =>
	fc.oneof(
		fc
			.record({
				artist: text(),
				title: text(),
				href: fc.oneof(
					fc.constant('/a/b?in=curator/sets/x'),
					fc.constant('/a/b'),
					fc.constant(null),
					fc.constant('https://soundcloud.com/a/b/'),
					fc.constant('not a url at all'),
					text()
				),
				artwork: fc.oneof(fc.constant(ART('x')), fc.constant(''), text()),
				system: fc.boolean()
			})
			.map(playlistRow),
		fc
			.record({
				artist: text(),
				title: text(),
				href: fc.oneof(fc.constant('/u/t'), fc.constant(null), text()),
				playlist: fc.boolean(),
				purchase: fc.option(fc.record({ url: fc.webUrl(), label: text() }), { nil: null })
			})
			.map(likedSound),
		fc
			.record({
				artist: text(),
				title: text(),
				href: fc.oneof(fc.constant('/u/t'), text()),
				icon: fc.boolean(),
				heading: fc.boolean()
			})
			.map(likedTile),
		fc.constant(tilePlaceholder()),
		// Rows the layout does not know about at all.
		text().map((junk) => `<li class="${junk}"><div>${junk}</div></li>`)
	);

const pageArb = () =>
	fc.oneof(
		fc
			.record({
				title: text(),
				curator: text(),
				rows: fc.array(rowArb(), { maxLength: 5 }),
				summary: fc.boolean(),
				trackList: fc.boolean(),
				engagement: fc.boolean()
			})
			.map(playlistPage),
		fc
			.record({ title: text(), rows: fc.array(rowArb(), { maxLength: 5 }), hero: fc.boolean() })
			.map(systemPlaylistPage),
		fc.record({ heading: text(), items: fc.array(rowArb(), { maxLength: 5 }) }).map(publicLikesPage),
		fc
			.record({ items: fc.array(rowArb(), { maxLength: 5 }), list: fc.boolean(), tabs: fc.boolean() })
			.map(ownLikesPage),
		fc.record({ title: text(), hero: fc.boolean(), engagement: fc.boolean() }).map(trackPage),
		// Not a SoundCloud page at all.
		fc.string({ unit: 'binary', maxLength: 400 })
	);

const pathArb = () =>
	fc.oneof(
		fc.constant('/curator/sets/my-playlist'),
		fc.constant('/you/likes'),
		fc.constant('/dj/likes'),
		fc.constant('/discover/sets/trending'),
		fc.constant('/'),
		// Not "//…": jsdom reads a protocol-relative path as another origin and
		// refuses to push it, which says nothing about the content script.
		fc.webPath().map((path) => (path.startsWith('//') ? `/x${path}` : path || '/'))
	);

// --- what an answer must always look like ------------------------------------

const PERMALINK = /^\/[^\s?#]+$/;

/** A permalink as a track carries one: a path as above, or '' for a title that is no link. */
const PERMALINK_OR_NONE = /^(?:\/[^\s?#]+)?$/;

function expectWellFormed(response: TracksAnswer, page: ParentNode) {
	expect(response).toBeTypeOf('object');
	expect(response.ok).toBe(true);
	expect(['playlist', 'likes', 'track', 'other']).toContain(response.pageType);
	expect(response.pageKey).toBeTypeOf('string');
	expect(response.title).toBeTypeOf('string');
	expect(response.curator).toBeTypeOf('string');
	expect(Number.isInteger(response.skippedPlaylists)).toBe(true);
	expect(response.skippedPlaylists).toBeGreaterThanOrEqual(0);
	expect(Array.isArray(response.tracks)).toBe(true);

	// A page type with no tracks of its own never carries a key.
	expect(response.pageType === 'other' ? response.pageKey : '').toBe('');

	for (const track of response.tracks) {
		expect(Object.keys(track).sort()).toEqual(expect.arrayContaining(['artist', 'artworkUrl', 'permalink', 'title']));

		for (const field of ['artist', 'title', 'artworkUrl', 'permalink'] as const) {
			expect(track[field], field).toBeTypeOf('string');
		}

		// A permalink is a path this extension can build a soundcloud.com URL
		// from — never a full URL, never a query string, never a stray space.
		expect(track.permalink).toMatch(PERMALINK_OR_NONE);
		// A cover is a web address, or '' for one that never loaded.
		expect(track.artworkUrl).toMatch(/^(?:$|https?:\/\/)/);
		// Whitespace is collapsed in every text field.
		expect(track.artist).not.toMatch(/\s\s|^\s|\s$/);
		expect(track.title).not.toMatch(/\s\s|^\s|\s$/);
		// Rows with nothing at all in them are dropped.
		expect(track.artist !== '' || track.title !== '').toBe(true);
	}

	// Never more tracks than there are places to read them from: a row each,
	// plus the hero, which is the one place a track page names its track.
	const candidates =
		page.querySelectorAll('li, .trackItem, .sound, .audibleTile, .playableTile').length +
		page.querySelectorAll('.l-listen-hero .fullHero').length;

	expect(response.tracks.length).toBeLessThanOrEqual(candidates);
}

// --- the properties ----------------------------------------------------------

describe('extraction answers for any page', () => {
	it('always answers, whatever the markup and the address bar say', async () => {
		// The checks are in expectWellFormed: this says they ran.
		expect.hasAssertions();
		await fc.assert(
			fc.asyncProperty(pageArb(), pathArb(), async (html, path) => {
				const body = render(html, path);
				expectWellFormed(await extract(), body);
			}),
			RUNS
		);
	});

	it('answers the same thing twice for the same page', async () => {
		await fc.assert(
			fc.asyncProperty(pageArb(), pathArb(), async (html, path) => {
				render(html, path);
				const first = await extract();
				const second = await extract();
				expect(second).toEqual(first);
			}),
			RUNS
		);
	});

	it('answers for markup cut off part-way through', async () => {
		// The checks are in expectWellFormed: this says they ran.
		expect.hasAssertions();

		const full = playlistPage({
			rows: [playlistRow({ artist: 'A', title: 'B' }), playlistRow({ artist: 'C', title: 'D' })]
		});

		await fc.assert(
			fc.asyncProperty(fc.nat({ max: full.length }), async (at) => {
				const body = render(full.slice(0, at), '/curator/sets/my-playlist');
				expectWellFormed(await extract(), body);
			}),
			{ numRuns: 120 }
		);
	});

	it('answers when the hydration blob is not the JSON it expects', async () => {
		// The checks are in expectWellFormed: this says they ran.
		expect.hasAssertions();
		await fc.assert(
			fc.asyncProperty(fc.string({ unit: 'binary', maxLength: 120 }), async (raw) => {
				const body = render(playlistPage({ rows: [playlistRow({})] }), '/c/sets/p');
				hydrationScript(null, { raw });
				expectWellFormed(await extract(), body);
			}),
			RUNS
		);
	});

	it('ignores messages it does not understand, whatever they are', async () => {
		await fc.assert(
			fc.asyncProperty(fc.jsonValue({ maxDepth: 2 }), async (message) => {
				render(playlistPage({ rows: [playlistRow({})] }), '/c/sets/p');
				expect(await chrome.testSendMessage(message)).toBeUndefined();
			}),
			RUNS
		);
	});
});

describe('the page changing under it', () => {
	it('paints markers through any run of mutations without throwing', async () => {
		await fc.assert(
			fc.asyncProperty(
				fc.array(
					fc.oneof(
						rowArb().map((row) => ({ add: row })),
						fc.constant({ removeFirst: true }),
						fc.constant({ clear: true }),
						text().map((junk) => ({ add: `<div>${junk}</div>` }))
					),
					{ maxLength: 8 }
				),
				async (steps: { add?: string; removeFirst?: true; clear?: true }[]) => {
					render(playlistPage({ rows: [playlistRow({ href: '/a/b' })] }), '/curator/sets/my-playlist');
					await chrome.storage.local.set({ downloaded: { '/a/b': '2026-01-01T00:00:00Z' } });
					const list = document.querySelector('.trackList__list') ?? document.body;

					for (const step of steps) {
						if (step.add) list.insertAdjacentHTML('beforeend', step.add);
						else if (step.removeFirst) list.firstElementChild?.remove();
						else list.replaceChildren();
						await vi.advanceTimersByTimeAsync(300);
					}

					// Still answering afterwards, and every marker it painted is one of
					// its own (the page's markup is never rewritten).
					expectWellFormed(await extract(), document.body);

					for (const mark of document.querySelectorAll<HTMLElement>('.tbx-mark')) {
						expect(['downloaded', 'nodl']).toContain(mark.dataset.kind);
					}
				}
			),
			{ numRuns: 40 }
		);
	});

	// A click anywhere at all, or on one of the few things a click marks a track
	// from: a row's More button and then the menu's download, or a purchase
	// link. A click on a random element almost never lands on one of those, and
	// the marks this checks are only ever made there.
	const MARKING = ['.sc-button-more', '.sc-button-download', '.sc-buylink'];

	const clickArb = () =>
		fc.oneof(
			{ arbitrary: fc.record({ selector: fc.constant('*'), nth: fc.nat({ max: 40 }) }), weight: 1 },
			{
				arbitrary: fc.record({ selector: fc.constantFrom(...MARKING), nth: fc.nat({ max: 4 }) }),
				weight: 3
			}
		);

	it('never throws on a click anywhere on the page, and marks only tracks', async () => {
		// The content script's own listener is capture-phase and lets the click
		// through; this one stops jsdom trying to follow the link afterwards,
		// which is the browser's business and not the script's.
		const stay = (event: Event) => event.preventDefault();
		document.addEventListener('click', stay);

		try {
			await fc.assert(
				fc.asyncProperty(pageArb(), fc.array(clickArb(), { minLength: 1, maxLength: 3 }), async (html, clicks) => {
					render(html, '/curator/sets/my-playlist');
					document.body.insertAdjacentHTML('beforeend', moreMenu());

					for (const { selector, nth } of clicks) {
						const elements = [...document.querySelectorAll(selector)];

						if (elements.length === 0) continue;
						elements[nth % elements.length].dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
						await vi.advanceTimersByTimeAsync(300);
					}

					// Any mark it made names a track, never a URL or a fragment of one.
					const marked = (await chrome.storage.local.get('downloaded')).downloaded ?? {};

					for (const permalink of Object.keys(marked)) expect(permalink).toMatch(PERMALINK);
				}),
				RUNS
			);
		} finally {
			document.removeEventListener('click', stay);
		}

		// And there were marks to check: clicks here did write some.
		const writes = chrome.storage.local.set.mock.calls.filter(([items]) => 'downloaded' in items);
		expect(writes.length).toBeGreaterThan(0);
	});
});

describe('download details for any page', () => {
	// A client id, so that the page's tracks really are asked about — and every
	// answer to every question is a failure. The script reads the page's
	// hydration once, as it starts, so it starts again over a page with one.
	beforeEach(async () => {
		disposeContentScripts();
		hydrationScript([apiClientEntry()]);
		await loadContentScript();
	});

	it('answers something a panel can render, even when nothing can be looked up', async () => {
		let asked = 0;
		await fc.assert(
			fc.asyncProperty(pageArb(), pathArb(), async (html, path) => {
				const api = mockFetch(() => ({ ok: false, status: 500, json: async () => ({}) }));
				render(html, path);
				const answer = chrome.testSendMessage({ type: 'GET_DOWNLOAD_DETAILS' });
				await vi.advanceTimersByTimeAsync(20000);
				const response = await answer;

				if (api.calls.length > 0) asked += 1;
				expect(response).toBeTypeOf('object');
				expect(response.ok).toBeTypeOf('boolean');
				expect(response).toBeOneOf([
					// Nothing came back, so the most a track can carry is what its own
					// row showed (a purchase link, marked partial), or nothing at all.
					expect.objectContaining({
						ok: true,
						tracks: expect.toSatisfy(
							(tracks: DetailedTrack[]) =>
								Array.isArray(tracks) &&
								tracks.every((track) => track.details === null || track.details?.partial === true)
						)
					}),
					// A failure names its cause, in `error` or in a `reason`.
					expect.objectContaining({ ok: false, error: expect.any(String) }),
					expect.objectContaining({ ok: false, reason: expect.any(String) })
				]);
			}),
			// Most runs ask nothing: a page with no tracks, or tracks an earlier run
			// already failed on (a failure is remembered). About one in seven does,
			// so this many runs all but always include several that did.
			{ numRuns: 80 }
		);
		expect(asked).toBeGreaterThan(0);
	});
});
