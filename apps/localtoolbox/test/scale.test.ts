// The sizes that actually turn up.
//
// The pages this runs on are not small: a curated playlist of five hundred
// tracks is ordinary, and a Likes page grows without limit. Two things go
// wrong at that size and at no other, and neither shows up in a test of three
// tracks.
//
// The first is cost to SoundCloud. Asking for one track at a time is
// invisible at three and rude at five hundred, so the request count for a full
// page is asserted against the batch size rather than against a number
// somebody typed: a change that quietly drops the batching fails here.
//
// The second is cost to the machine, and the assertions about it count work
// rather than time: requests made, writes performed, searches of the DOM,
// markers painted. (Rows drawn are counted where the Manager panel is tested:
// test/panels/manager.test.ts, "draws a screenful first…".) A stopwatch would measure whatever else the
// machine is doing; a count of how many times the page was searched for eight
// hundred rows against one hundred is the same number every time, and is
// exactly what goes wrong when a scan turns quadratic. The two wall-clock
// budgets left, over pure functions at five thousand items, have three orders
// of magnitude of headroom and are there to catch a change from linear to
// quadratic, which at that size is minutes rather than milliseconds.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { loadContentScript, disposeContentScripts } from './helpers/load.ts';
import { mockFetch, jsonResponse, likesListing } from './helpers/api.ts';
import { flush, until } from './helpers/flush.ts';
import {
	render,
	playlistPage,
	playlistRow,
	publicLikesPage,
	likedSound,
	hydrationScript,
	apiTrack,
	apiClientEntry,
	CLIENT_ID
} from './helpers/dom.ts';
import { FORMATS, processTracks } from '../src/tools/soundcloud-playlist-to-text/formats.ts';
import { buildGroups, buildListOrder } from '../src/tools/manager/links.ts';
import type { ChromeMock } from './helpers/chrome.ts';
import type { DetailsAnswer, PageFailure } from '$lib/types';
import { knownDetails, listedEntry } from './helpers/state-fixtures.ts';

declare const chrome: ChromeMock;

const SET = '/curator/sets/big';

const API_BATCH = 50; // content.js: ids per /tracks request

const LIKES_PAGE = 200; // content.js: likes per listing request

afterEach(disposeContentScripts);

const permalink = (n: number) => `/a${n}/t${n}`;

/** The tracks a details answer carries; a page that failed has none to give. */
const tracksOf = (answer: DetailsAnswer | PageFailure) => (answer.ok ? answer.tracks : []);

const bigPlaylist = (n: number) =>
	playlistPage({
		title: 'Big',
		rows: Array.from({ length: n }, (_, i) =>
			playlistRow({ artist: `A${i}`, title: `T${i}`, href: `${permalink(i)}?in=curator/sets/big` })
		)
	});

// How many times the DOM was searched while `work` ran.
//
// A stopwatch is the obvious way to ask whether something scales, and the
// wrong one here: the answer depends on what else the machine is doing, which
// makes a threshold either useless or flaky. Counting the searches instead is
// exact, is the same number on every machine, and is the thing that actually
// grows when a scan turns quadratic.
async function searches<Value>(work: () => Promise<Value>) {
	const spies = [
		vi.spyOn(Element.prototype, 'querySelector'),
		vi.spyOn(Element.prototype, 'querySelectorAll'),
		vi.spyOn(Element.prototype, 'closest'),
		vi.spyOn(Element.prototype, 'matches'),
		vi.spyOn(Document.prototype, 'querySelector'),
		vi.spyOn(Document.prototype, 'querySelectorAll')
	];

	try {
		const value = await work();

		return { value, count: spies.reduce((count, spy) => count + spy.mock.calls.length, 0) };
	} finally {
		for (const spy of spies) spy.mockRestore();
	}
}

describe('reading a large playlist', () => {
	// Nothing here waits on a timer, but the script sets one: a page rendered
	// under a running script is a mutation, and the repaint it schedules is a
	// real 150 ms timeout that disposing the script cannot reach. Left real, it
	// fired in whichever test ran next and repainted that test's page from this
	// one's empty marks — taking every marker off it. Faked, it is thrown away
	// with the fake clock when this test ends (test/setup.js).
	beforeEach(() => {
		vi.useFakeTimers();
	});

	it('reads five hundred rows, in order, with nothing dropped or repeated', async () => {
		render(bigPlaylist(500), SET);
		await loadContentScript();

		const response = await chrome.testSendMessage({ type: 'EXTRACT_TRACKS' });
		expect(response.tracks).toHaveLength(500);
		expect(response.tracks.map((t) => t.permalink)).toEqual(Array.from({ length: 500 }, (_, i) => permalink(i)));
		expect(new Set(response.tracks.map((t) => t.permalink)).size).toBe(500);
	});

	// The shape of the cost, not its size: eight times the rows must not be
	// sixty-four times the work.
	it('searches the page a number of times the row count explains', async () => {
		render(bigPlaylist(100), SET);
		await loadContentScript();
		const small = await searches(() => chrome.testSendMessage({ type: 'EXTRACT_TRACKS' }));

		render(bigPlaylist(800), SET);
		const large = await searches(() => chrome.testSendMessage({ type: 'EXTRACT_TRACKS' }));

		expect(large.value.tracks).toHaveLength(800);
		expect(small.count).toBeGreaterThan(0);
		// Three searches per row plus a fixed handful for the hero and the list:
		// 307 for a hundred rows, 2407 for eight hundred, a ratio of 7.8 where
		// linear is 8. A row scan that looked at every other row would be 64.
		expect(large.count).toBeLessThanOrEqual(small.count * 9);
	});
});

describe('looking five hundred tracks up', () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});

	it('asks in batches of fifty and nothing more', async () => {
		const total = 500;
		render(bigPlaylist(total), SET);
		hydrationScript([
			apiClientEntry(),
			{
				hydratable: 'playlist',
				data: {
					kind: 'playlist',
					permalink_url: `https://soundcloud.com${SET}`,
					// As SoundCloud serves them: a handful of full tracks, the rest
					// stubs carrying nothing but an id.
					tracks: Array.from({ length: total }, (_, i) =>
						i < 5 ? apiTrack({ id: i, permalink: permalink(i) }) : { id: i }
					)
				}
			}
		]);

		const { calls, paths } = mockFetch((url) =>
			jsonResponse(
				(url.searchParams.get('ids') ?? '')
					.split(',')
					.filter(Boolean)
					.map((id) => apiTrack({ id: Number(id), permalink: permalink(Number(id)) }))
			)
		);

		await loadContentScript();
		const answer = chrome.testSendMessage({ type: 'GET_DOWNLOAD_DETAILS' });
		await vi.advanceTimersByTimeAsync(20000);
		const response = await answer;

		expect(tracksOf(response)).toHaveLength(total);
		expect(tracksOf(response).every((t) => t.details)).toBe(true);

		// Ten requests for five hundred tracks, and not one /resolve: everything
		// came from the page's own hydration and the bulk endpoint.
		const batches = paths().filter((path) => path.startsWith('/tracks?'));
		expect(batches).toHaveLength(Math.ceil((total - 5) / API_BATCH));
		expect(paths().filter((path) => path.startsWith('/resolve'))).toEqual([]);
		expect(calls).toHaveLength(batches.length);

		for (const call of calls) {
			expect(call.url.searchParams.get('ids')!.split(',').length).toBeLessThanOrEqual(API_BATCH);
			expect(call.url.searchParams.get('client_id')).toBe(CLIENT_ID);
		}

		// And the cache is written in a few batched writes, not one per track.
		expect(chrome.storage.local.set.mock.calls.length).toBeLessThan(total / 10);
		expect(Object.keys(chrome.testStore().details)).toHaveLength(total);
	});

	it('pages a long Likes page at the API’s maximum', async () => {
		const total = 450;
		render(
			publicLikesPage({
				user: 'dj',
				items: Array.from({ length: total }, (_, i) =>
					likedSound({ artist: `A${i}`, title: `T${i}`, href: permalink(i) })
				)
			}),
			'/dj/likes'
		);
		hydrationScript([apiClientEntry(), { hydratable: 'user', data: { id: 7, permalink: 'dj' } }]);

		const page = (from: number, size: number) =>
			Array.from({ length: size }, (_, i) => apiTrack({ id: from + i, permalink: permalink(from + i) }));

		const { paths } = mockFetch((url) => {
			if (!url.pathname.startsWith('/users/7/likes')) return undefined;
			const offset = Number(url.searchParams.get('offset') ?? 0);
			const size = Math.min(LIKES_PAGE, total - offset);
			const next = offset + size < total ? `https://api-v2.soundcloud.com/users/7/likes?offset=${offset + size}` : null;

			return jsonResponse(likesListing(page(offset, size), next));
		});

		await loadContentScript();
		const answer = chrome.testSendMessage({ type: 'GET_DOWNLOAD_DETAILS' });
		await vi.advanceTimersByTimeAsync(20000);
		const response = await answer;

		expect(tracksOf(response)).toHaveLength(total);
		const listings = paths().filter((path) => path.startsWith('/users/7/likes'));
		expect(listings).toHaveLength(Math.ceil(total / LIKES_PAGE));
		// Nothing was resolved one at a time afterwards.
		expect(paths().filter((path) => path.startsWith('/resolve'))).toEqual([]);
	});
});

describe('painting markers over a large page', () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});

	it('marks only the rows that are marked, once each, in one pass', async () => {
		const marked = Object.fromEntries(
			Array.from({ length: 120 }, (_, i) => [permalink(i * 2), '2026-01-01T00:00:00.000Z'])
		);

		render(bigPlaylist(500), SET);
		await loadContentScript();
		await chrome.storage.local.set({ downloaded: marked });
		await vi.advanceTimersByTimeAsync(500);
		await flush();
		// The marks arrive on a storage event, and the repaint the page's own
		// mutations schedule follows them; both are waited for rather than
		// assumed to have happened within a fixed number of turns. What this test
		// is really about is asserted below whichever way the waiting goes: the
		// count, the kind, one marker per row rather than one per repaint, and a
		// second pass changing nothing.
		await until(() => document.querySelectorAll('.tbx-mark').length === 120, { ms: 10000 });

		const marks = document.querySelectorAll<HTMLElement>('.tbx-mark');
		expect(marks).toHaveLength(120);

		for (const mark of marks) expect(mark.dataset.kind).toBe('downloaded');
		// One marker per row, not one per repaint.
		const rowsWithMarks = new Set([...marks].map((mark) => mark.closest('.trackItem')));
		expect(rowsWithMarks.size).toBe(120);
		// And the rows are the marked ones: every other track from the first,
		// not any hundred and twenty rows the count happens to come to.
		const titles = [...rowsWithMarks].map((row) => row!.querySelector('.trackItem__trackTitle')!.textContent);
		expect(titles.sort()).toEqual(Array.from({ length: 120 }, (_, i) => `T${i * 2}`).sort());

		// A second pass over the same page changes nothing.
		await chrome.storage.local.set({ downloaded: marked });
		await vi.advanceTimersByTimeAsync(500);
		await flush();
		expect(document.querySelectorAll('.tbx-mark')).toHaveLength(120);
	});
});

describe('the pure logic at size', () => {
	const many = (n: number) => Array.from({ length: n }, (_, i) => ({ artist: `Artist ${i}`, title: `Title - ${i}` }));

	it('renders every format for five thousand tracks in well under a second', () => {
		const tracks = many(5000);

		for (const format of FORMATS) {
			const started = performance.now();
			const out = format.render(tracks);
			expect(performance.now() - started, format.value).toBeLessThan(1000);
			expect(out.length).toBeGreaterThan(1000);
		}
	});

	it('dedupes and splits five thousand tracks without pairwise comparison', () => {
		const tracks = [...many(2500), ...many(2500)];
		const started = performance.now();
		const out = processTracks(tracks, { splitTitle: true, dedupe: true });
		expect(performance.now() - started).toBeLessThan(1000);
		expect(out).toHaveLength(2500);
	});

	it('groups five thousand links in one pass', () => {
		const tracks = Array.from({ length: 5000 }, (_, i) =>
			listedEntry({
				artist: `A${i}`,
				title: `T${i}`,
				permalink: permalink(i),
				details: knownDetails({
					permalink: permalink(i),
					downloadable: i % 2 === 0,
					hasDownloadsLeft: true,
					purchaseUrl: i % 3 === 0 ? `https://site${i % 20}.com/x` : '',
					purchaseTitle: ''
				})
			})
		);

		const started = performance.now();
		const groups = buildGroups(tracks);
		const order = buildListOrder(tracks);
		expect(performance.now() - started).toBeLessThan(1000);
		expect(groups.length).toBeLessThanOrEqual(21);
		// Every track, not only the ones with a link: the list draws what it holds.
		expect(order).toHaveLength(5000);
	});
});
