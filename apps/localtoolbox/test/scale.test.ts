// The sizes that turn up.
//
// The pages this runs on are not small: a curated playlist of five hundred
// tracks is ordinary, and a Likes page grows without limit. Two things go
// wrong at that size and at no other, and neither shows up in a test of three
// tracks.
//
// The first is what the page has loaded. A list that long arrives in pieces
// (a playlist's first few tracks whole and the rest as stubs, then the page's
// own requests for the rest in batches), and every row has to come out with
// its details whatever piece they were in.
//
// The second is cost to the machine, and the assertions about it count work
// rather than time: writes performed, searches of the DOM,
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
import { flush, until } from './helpers/flush.ts';
import {
	render,
	playlistPage,
	playlistRow,
	publicLikesPage,
	likedSound,
	hydrationScript,
	apiTrack,
	pageLoaded
} from './helpers/dom.ts';
import { FORMATS, processTracks } from '../src/tools/soundcloud-playlist-to-text/formats.ts';
import { buildGroups, buildListOrder } from '../src/tools/manager/links.ts';
import type { ChromeMock } from './helpers/chrome.ts';
import { knownDetails, listedEntry } from './helpers/state-fixtures.ts';

declare const chrome: ChromeMock;

const SET = '/curator/sets/big';

// How SoundCloud's web app asks for tracks it has only stubs of, and how many
// likes one of its listing requests brings. Neither is this extension's
// choice: they are the sizes the page's own answers come in.
const API_BATCH = 50;

const LIKES_PAGE = 200;

afterEach(disposeContentScripts);

const permalink = (n: number) => `/a${n}/t${n}`;

const track = (i: number) => apiTrack({ id: i, permalink: permalink(i) });

// `items` cut into pieces of `size`, as the page asks for them.
const piecesOf = <Item>(items: Item[], size: number) =>
	Array.from({ length: Math.ceil(items.length / size) }, (_, i) => items.slice(i * size, (i + 1) * size));

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
// exact, is the same number on every machine, and is the thing that grows
// when a scan turns quadratic.
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
	// would fire in whichever test ran next and repaint that test's page from
	// this one's empty marks, taking every marker off it. Faked, it is thrown
	// away with the fake clock when this test ends (test/setup.ts).
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

describe('details for five hundred tracks', () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});

	it('answers for every row of a playlist the page loaded in pieces, keeping none of it in storage', async () => {
		const total = 500;
		render(bigPlaylist(total), SET);
		hydrationScript([
			{
				hydratable: 'playlist',
				data: {
					kind: 'playlist',
					permalink_url: `https://soundcloud.com${SET}`,
					// As SoundCloud serves them: a handful of full tracks, the rest
					// stubs carrying nothing but an id.
					tracks: Array.from({ length: total }, (_, i) => (i < 5 ? track(i) : { id: i }))
				}
			}
		]);
		await loadContentScript();

		// The page fetches the stubs itself, fifty at a time, as it scrolls.
		const rest = Array.from({ length: total - 5 }, (_, i) => track(i + 5));

		for (const piece of piecesOf(rest, API_BATCH)) pageLoaded('/tracks', piece);
		await vi.advanceTimersByTimeAsync(500);

		const response = await chrome.testSendMessage({ type: 'GET_DOWNLOAD_DETAILS' });
		expect(response.tracks).toHaveLength(total);
		// Each row's own, by the id SoundCloud gave it: none missing, none crossed.
		expect(response.tracks.map((t) => t.details?.id)).toEqual(Array.from({ length: total }, (_, i) => i));
		// Learned from the page, and kept in the page: not one write of details,
		// however many there are.
		expect(chrome.storage.local.set.mock.calls.filter(([items]) => 'details' in items)).toEqual([]);
	});

	it('answers for every row of a long Likes page from the listings the page loaded', async () => {
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
		await loadContentScript();

		const likes = Array.from({ length: total }, (_, i) => ({ track: track(i), created_at: '2026-01-01T00:00:00Z' }));

		for (const piece of piecesOf(likes, LIKES_PAGE)) pageLoaded('/users/7/track_likes', { collection: piece });

		const response = await chrome.testSendMessage({ type: 'GET_DOWNLOAD_DETAILS' });
		expect(response.tracks).toHaveLength(total);
		expect(response.tracks.map((t) => t.details?.id)).toEqual(Array.from({ length: total }, (_, i) => i));
	});

	// The answer is read off the page and out of memory, so its cost is the
	// row count's: eight times the rows must not be sixty-four times the work.
	it('searches the page a number of times the row count explains', async () => {
		const known = (n: number) =>
			pageLoaded(
				'/tracks',
				Array.from({ length: n }, (_, i) => track(i))
			);

		render(bigPlaylist(100), SET);
		await loadContentScript();
		known(100);
		const small = await searches(() => chrome.testSendMessage({ type: 'GET_DOWNLOAD_DETAILS' }));

		render(bigPlaylist(800), SET);
		known(800);
		const large = await searches(() => chrome.testSendMessage({ type: 'GET_DOWNLOAD_DETAILS' }));

		expect(small.value.tracks.every((t) => t.details)).toBe(true);
		expect(large.value.tracks).toHaveLength(800);
		expect(large.value.tracks.every((t) => t.details)).toBe(true);
		expect(small.count).toBeGreaterThan(0);
		expect(large.count).toBeLessThanOrEqual(small.count * 9);
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
		// is about is asserted below whichever way the waiting goes: the
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
