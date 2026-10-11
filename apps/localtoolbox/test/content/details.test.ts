// content.ts — download details (the GET_DOWNLOAD_DETAILS message): the
// hydration data, the details cache, the bulk API paths for playlists and
// Likes pages, the per-track fallback, and the one-lookup-at-a-time queue.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import { loadContentScript, disposeContentScripts } from '../helpers/load.ts';
import { mockFetch, jsonResponse, likesListing, type Route } from '../helpers/api.ts';
import { flush } from '../helpers/flush.ts';
import { createChrome, type ChromeMock, type Json } from '../helpers/chrome.ts';
import {
	ART,
	render,
	playlistPage,
	playlistRow,
	publicLikesPage,
	ownLikesPage,
	likedSound,
	likedTile,
	hydrationScript,
	apiTrack,
	apiClientEntry,
	CLIENT_ID,
	type ApiTrackOptions,
	type LikedSoundOptions
} from '../helpers/dom.ts';
import type { DetailsAnswer, KnownDetails } from '$lib/types';

declare const chrome: ChromeMock;

const NOW = new Date('2026-09-08T12:00:00Z').getTime();

const DAY = 24 * 60 * 60 * 1000;

const API_CONCURRENCY = 4; // content.js: requests of one kind in the air at once

const CACHE_MAX = 20000; // content.js: tracks the stored cache keeps

beforeEach(() => {
	vi.useFakeTimers();
});

afterEach(disposeContentScripts);

const SET = '/curator/sets/my-playlist';

const row = (n: number) =>
	playlistRow({ artist: `A${n}`, title: `T${n}`, href: `/a${n}/t${n}?in=curator/sets/my-playlist` });

const permalink = (n: number) => `/a${n}/t${n}`;

const track = (n: number, extra: ApiTrackOptions = {}) => apiTrack({ id: n, permalink: permalink(n), ...extra });

const playlistEntry = (tracks: Json[], path = SET) => ({
	hydratable: 'playlist',
	data: { kind: 'playlist', permalink_url: `https://soundcloud.com${path}`, tracks }
});

const askDetails = () => chrome.testSendMessage({ type: 'GET_DOWNLOAD_DETAILS' });

/** The tracks' details: a failed lookup, which no test reading them expects, fails here. */
const getDetails = async () => {
	const answer = await askDetails();

	if (!answer.ok) throw new Error(`the lookup failed: ${answer.error}`);

	return answer;
};

// On a playlist page whose hydration does not describe the playlist, a lookup
// resolves the playlist's own URL first; answering that with something else
// sends the lookup straight on to resolving the rows one by one.
const isPlaylistResolve = (url: URL) =>
	url.pathname === '/resolve' && (url.searchParams.get('url') || '').includes('/sets/');

const notThePlaylist =
	(handler: Route): Route =>
	(url, init, n) =>
		isPlaylistResolve(url) ? jsonResponse({ kind: 'user', id: 1 }) : handler(url, init, n);

const trackNumber = (url: URL) => Number(/t(\d+)$/.exec(url.searchParams.get('url')!)![1]);

const storedDetails = () => chrome.testStore().details;

const detailsOf = (response: DetailsAnswer) => Object.fromEntries(response.tracks.map((t) => [t.permalink, t.details]));

describe('before any lookup', () => {
	it('answers an empty page without needing the API key', async () => {
		render('<p>nothing</p>', '/stream');
		await loadContentScript();
		const response = await getDetails();
		expect(response).toMatchObject({ ok: true, tracks: [], lookupFailed: 0, resumed: false, error: '' });
	});

	it('reports no-client-id when the page carries no hydration data', async () => {
		render(playlistPage({ rows: [row(1)] }), SET);
		const { fn } = mockFetch();
		await loadContentScript();
		const response = await getDetails();
		expect(response.error).toBe('no-client-id');
		expect(response.tracks).toHaveLength(1);
		expect(response.tracks[0].details).toBeUndefined();
		expect(fn).not.toHaveBeenCalled();
	});

	it.each([
		['no assignment', 'var x = 1;'],
		['invalid JSON', 'window.__sc_hydration = [{oops};'],
		['not an array', 'window.__sc_hydration = {"hydratable":"apiClient"};'],
		['a null client entry', 'window.__sc_hydration = [null, {"hydratable":"apiClient","data":{"id":123}}];'],
		['a client entry without data', 'window.__sc_hydration = [{"hydratable":"apiClient"}];']
	])('reports no-client-id for hydration with %s', async (_name, raw) => {
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript(null, { raw });
		await loadContentScript();
		expect((await getDetails()).error).toBe('no-client-id');
	});

	it('ignores external scripts and reads the first inline hydration, even with trailing markup', async () => {
		render(playlistPage({ rows: [row(1)] }), SET);
		const external = document.createElement('script');
		external.src = 'https://a-v2.sndcdn.com/assets/app.js';
		external.textContent = 'window.__sc_hydration = [{"hydratable":"apiClient","data":{"id":"wrong"}}];';
		document.head.append(external);
		hydrationScript(null, {
			raw: `\n  window.__sc_hydration = [{"hydratable":"apiClient","data":{"id":"${CLIENT_ID}"}}];\n</script><script>other();`
		});
		hydrationScript([apiClientEntry('second-script')]);
		const { calls } = mockFetch(() => jsonResponse(track(1)));
		await loadContentScript();
		const response = await getDetails();
		expect(response.error).toBe('');
		expect(calls[0].url.searchParams.get('client_id')).toBe(CLIENT_ID);
	});
});

describe('the cache', () => {
	// A stored entry of the shape this version writes. Every field detailsOf
	// keeps has to be there: an entry missing one was written by an older
	// version, and is stale whatever its age (see hasEveryField in content.ts).
	const stored1 = (permalinkPath: string, extra: Partial<KnownDetails> = {}) => ({
		id: 1,
		permalink: permalinkPath,
		downloadable: false,
		hasDownloadsLeft: true,
		purchaseUrl: '',
		purchaseTitle: '',
		genre: '',
		artworkUrl: '',
		at: NOW - DAY,
		...extra
	});

	it('answers from fresh stored details without asking SoundCloud', async () => {
		const cached = {
			id: 1,
			permalink: permalink(1),
			downloadable: true,
			hasDownloadsLeft: true,
			purchaseUrl: '',
			purchaseTitle: '',
			genre: '',
			artworkUrl: '',
			at: NOW - DAY
		};

		globalThis.chrome = createChrome({ storage: { details: { [permalink(1)]: cached } } });
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry()]);
		const { fn } = mockFetch();
		await loadContentScript();
		const response = await getDetails();
		expect(fn).not.toHaveBeenCalled();
		expect(response.tracks[0].details).toEqual(cached);
		expect(response).toMatchObject({ lookupFailed: 0, resumed: false, error: '' });
	});

	// The one that matters most about hasEveryField, because the failure it
	// guards against is silent and total: if the list of fields it checks ever
	// names one detailsOf does not write, then every entry the extension writes
	// fails its own check, nothing is ever read back out of the cache, and every
	// page asks SoundCloud about every track for ever. Nothing else here would
	// notice — the panel would look exactly right.
	//
	// So this is the round trip, through the real code both ways: learn a track,
	// let the cache be written, load the extension again on the same storage and
	// require that it asks nobody.
	it('reads back what it wrote: a learned entry needs no second request', async () => {
		globalThis.chrome = createChrome();
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry()]);
		const { fn } = mockFetch(notThePlaylist(() => jsonResponse(track(1, { downloadable: true, genre: 'Techno' }))));
		await loadContentScript();
		expect((await getDetails()).tracks[0].details).toMatchObject({ genre: 'Techno' });
		await flush();
		const calls = fn.mock.calls.length;
		expect(calls).toBeGreaterThan(0);

		// A second visit, on the cache the first one left behind.
		disposeContentScripts();
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry()]);
		await loadContentScript();

		const response = await getDetails();
		expect(fn.mock.calls.length).toBe(calls);
		expect(response.tracks[0].details).toMatchObject({ downloadable: true, genre: 'Techno' });
	});

	// Age is not the only way an entry goes off. The details kept here have
	// grown, and an entry from before a field was kept will never gain it by
	// waiting: nothing would ask about the track again for a month, and the
	// panel would say it did not know the genre of a track SoundCloud was about
	// to name the genre of anyway.
	it('asks again about entries an older version wrote, and keeps what comes back', async () => {
		const { genre: _dropped, ...beforeTheGenre } = stored1(permalink(1), { downloadable: true });
		globalThis.chrome = createChrome({ storage: { details: { [permalink(1)]: beforeTheGenre } } });
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry()]);
		const { fn } = mockFetch(notThePlaylist(() => jsonResponse(track(1, { downloadable: true, genre: 'Techno' }))));
		await loadContentScript();

		const response = await getDetails();
		expect(fn).toHaveBeenCalled();
		expect(response.tracks[0].details).toMatchObject({ genre: 'Techno', at: NOW });
		expect(response.lookupFailed).toBe(0);

		// And the entry that replaced it is one a later run will accept.
		await flush();
		expect(storedDetails()[permalink(1)]).toMatchObject({ genre: 'Techno' });
	});

	// The hydration harvest runs on every page load and costs nothing, so most
	// of that re-asking never becomes a request at all.
	it('refills an older version’s entry from the page itself, without asking', async () => {
		const { genre: _dropped, ...beforeTheGenre } = stored1(permalink(1), { downloadable: true });
		globalThis.chrome = createChrome({ storage: { details: { [permalink(1)]: beforeTheGenre } } });
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry(), playlistEntry([track(1, { downloadable: true, genre: 'Drum & Bass' })])]);
		const { fn } = mockFetch();
		await loadContentScript();

		const response = await getDetails();
		expect(fn).not.toHaveBeenCalled();
		expect(response.tracks[0].details).toMatchObject({ genre: 'Drum & Bass' });
	});

	// A remembered failure has none of those fields and is not meant to: its own
	// day-long lifetime is what expires it.
	it('leaves remembered failures alone', async () => {
		globalThis.chrome = createChrome({
			storage: { details: { [permalink(1)]: { unavailable: true, at: NOW - DAY / 2 } } }
		});
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry()]);
		const { fn } = mockFetch();
		await loadContentScript();

		const response = await getDetails();
		expect(fn).not.toHaveBeenCalled();
		expect(response.tracks[0].details).toBeNull();
		expect(response.lookupFailed).toBe(0);
	});

	it('ignores expired, malformed and stale-failure entries, and does not count old failures', async () => {
		globalThis.chrome = createChrome({
			storage: {
				details: {
					[permalink(1)]: { id: 1, permalink: permalink(1), downloadable: false, purchaseUrl: '', at: NOW - 31 * DAY },
					[permalink(2)]: { id: 2, permalink: permalink(2), downloadable: false, purchaseUrl: '', at: 'yesterday' },
					[permalink(3)]: { unavailable: true, at: NOW - 2 * DAY },
					[permalink(4)]: { unavailable: true, at: NOW - DAY / 2 },
					'/kept/elsewhere': stored1('/kept/elsewhere', { id: 9, downloadable: true })
				}
			}
		});
		render(playlistPage({ rows: [row(1), row(2), row(3), row(4)] }), SET);
		hydrationScript([apiClientEntry()]);
		const { paths } = mockFetch(notThePlaylist((url) => jsonResponse(track(trackNumber(url), { downloadable: true }))));
		await loadContentScript();
		const response = await getDetails();
		expect(paths().map((p) => p.replace(/&client_id=.*$/, ''))).toEqual([
			`/resolve?url=${encodeURIComponent(`https://soundcloud.com${SET}`)}`,
			`/resolve?url=${encodeURIComponent(`https://soundcloud.com${permalink(1)}`)}`,
			`/resolve?url=${encodeURIComponent(`https://soundcloud.com${permalink(2)}`)}`,
			`/resolve?url=${encodeURIComponent(`https://soundcloud.com${permalink(3)}`)}`
		]);
		const details = detailsOf(response);
		expect(details[permalink(1)]!.downloadable).toBe(true);
		expect(details[permalink(3)]!.downloadable).toBe(true);
		expect(details[permalink(4)]).toBeNull();
		expect(response.lookupFailed).toBe(0);
		await flush();
		const stored = storedDetails();
		expect(Object.keys(stored).sort()).toEqual(
			[permalink(1), permalink(2), permalink(3), permalink(4), '/kept/elsewhere'].sort()
		);
		expect(stored[permalink(1)].at).toBe(NOW);
	});

	it('writes what it learns back a moment later, merged with other tabs’ entries', async () => {
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([
			apiClientEntry(),
			playlistEntry([track(1, { purchaseUrl: 'https://x.test/1', purchaseTitle: ' Buy ', genre: ' Drum & Bass ' })])
		]);
		mockFetch();
		await loadContentScript();
		expect(storedDetails()).toBeUndefined();

		// Another tab writes meanwhile; the write-back keeps its entry. It is
		// staged where only the write-back's own read finds it: written through
		// storage, it would reach this tab's cache by the change event instead,
		// and be kept whether the write-back merged or not.
		const other = {
			id: 5,
			permalink: '/other/tab',
			downloadable: false,
			hasDownloadsLeft: true,
			purchaseUrl: '',
			purchaseTitle: '',
			genre: '',
			artworkUrl: '',
			at: NOW
		};

		const realGet = chrome.storage.local.get.getMockImplementation()!;
		chrome.storage.local.get.mockImplementation(async (keys) => {
			const found = await realGet(keys);

			if (keys === 'details') found.details = Object.assign({}, found.details, { '/other/tab': other });

			return found;
		});
		await vi.advanceTimersByTimeAsync(1000);
		const stored = storedDetails();
		expect(stored[permalink(1)]).toEqual({
			id: 1,
			permalink: permalink(1),
			downloadable: false,
			hasDownloadsLeft: true,
			purchaseUrl: 'https://x.test/1',
			purchaseTitle: 'Buy',
			genre: 'Drum & Bass',
			artworkUrl: '',
			at: NOW
		});
		expect(stored['/other/tab']).toEqual(other);
	});

	it('survives a storage that refuses the write-back', async () => {
		render(playlistPage({ rows: [row(1), row(2)] }), SET);
		hydrationScript([apiClientEntry(), playlistEntry([track(1)])]);
		mockFetch(notThePlaylist(() => jsonResponse(track(2))));
		await loadContentScript();
		chrome.storage.local.set.mockRejectedValue(new Error('QUOTA_BYTES exceeded'));
		// Scheduled write-back (after the harvest) …
		await vi.advanceTimersByTimeAsync(1000);
		// … and the immediate one when a lookup ends.
		const response = await getDetails();
		expect(response.ok).toBe(true);
		await flush();
		expect(chrome.storage.local.set).toHaveBeenCalled();
	});

	it('takes newer details another tab stored, and lets them settle a waiting request', async () => {
		render(playlistPage({ rows: [row(1), row(2)] }), SET);
		hydrationScript([apiClientEntry()]);
		// SoundCloud never answers in this tab.
		mockFetch(() => new Promise(() => {}));
		await loadContentScript();
		const pending = getDetails();
		await flush();

		const fromOtherTab = (n: number, at: number) => ({
			id: n,
			permalink: permalink(n),
			downloadable: true,
			hasDownloadsLeft: true,
			purchaseUrl: '',
			purchaseTitle: '',
			genre: '',
			artworkUrl: '',
			at
		});

		chrome.testEmitStorageChange({
			details: {
				newValue: {
					[permalink(1)]: fromOtherTab(1, NOW),
					[permalink(2)]: fromOtherTab(2, NOW - 40 * DAY) // expired: ignored
				}
			}
		});
		await flush();
		chrome.testEmitStorageChange({ details: { newValue: { [permalink(2)]: fromOtherTab(2, NOW + 1) } } });
		const response = await pending;
		expect(detailsOf(response)[permalink(1)]).toMatchObject({ at: NOW });
		expect(detailsOf(response)[permalink(2)]).toMatchObject({ at: NOW + 1 });
		// An older copy of something already known changes nothing.
		chrome.testEmitStorageChange({ details: { newValue: { [permalink(2)]: fromOtherTab(2, NOW - 1) } } });
		chrome.testEmitStorageChange({ details: { newValue: undefined } });
		chrome.testEmitStorageChange({ downloaded: { newValue: {} } }, 'sync');
		await flush();
		expect((await getDetails()).tracks[1].details).toMatchObject({ at: NOW + 1 });
	});

	// The cap is what keeps the cache writable. An area Chrome will not take is
	// an area that is empty next visit, and an empty cache asks SoundCloud about
	// every track on every page again — so forgetting the oldest entries is the
	// cheap outcome and refusing to forget is the expensive one.
	it('caps the stored cache, keeping the entries learned most recently', async () => {
		const old = Object.fromEntries(
			Array.from({ length: CACHE_MAX }, (_, i) => [
				`/old/t${i}`,
				{
					id: i,
					permalink: `/old/t${i}`,
					downloadable: false,
					hasDownloadsLeft: true,
					purchaseUrl: '',
					purchaseTitle: '',
					genre: '',
					artworkUrl: '',
					at: NOW - DAY - i
				}
			])
		);

		globalThis.chrome = createChrome({ storage: { details: old } });
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry(), playlistEntry([track(1)])]);
		mockFetch();
		await loadContentScript();
		await vi.advanceTimersByTimeAsync(1000);

		const stored = storedDetails();
		expect(Object.keys(stored)).toHaveLength(CACHE_MAX);
		// The track just learned is in; the oldest entry made room for it.
		expect(stored[permalink(1)]).toBeDefined();
		expect(stored[`/old/t${CACHE_MAX - 1}`]).toBeUndefined();
		expect(stored['/old/t0']).toBeDefined();
	});

	it('writes half as much rather than nothing when the area refuses the capped write', async () => {
		const old = Object.fromEntries(
			Array.from({ length: CACHE_MAX }, (_, i) => [
				`/old/t${i}`,
				{
					id: i,
					permalink: `/old/t${i}`,
					downloadable: false,
					hasDownloadsLeft: true,
					purchaseUrl: '',
					purchaseTitle: '',
					genre: '',
					artworkUrl: '',
					at: NOW - DAY - i
				}
			])
		);

		globalThis.chrome = createChrome({ storage: { details: old } });
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry(), playlistEntry([track(1)])]);
		mockFetch();
		await loadContentScript();
		chrome.storage.local.set.mockRejectedValueOnce(new Error('QUOTA_BYTES exceeded'));
		await vi.advanceTimersByTimeAsync(1000);

		const stored = storedDetails();
		expect(Object.keys(stored)).toHaveLength(CACHE_MAX / 2);
		expect(stored[permalink(1)]).toBeDefined();
	});
});

describe('playlist pages', () => {
	it('harvests the hydration’s full tracks and fetches the stubs in batches only as far as needed', async () => {
		render(playlistPage({ rows: [row(1), row(2), row(3)] }), SET);
		const stubs = Array.from({ length: 70 }, (_, i) => ({ id: i + 2 }));
		hydrationScript([
			{ hydratable: 'anonymousId', data: 'abc' },
			null,
			{ hydratable: 'broken', data: null },
			{ hydratable: 'sound', data: { kind: 'track' } }, // no permalink: dropped
			{ hydratable: 'user', data: { kind: 'user', permalink_url: 'https://soundcloud.com/curator', tracks: 'n/a' } },
			apiClientEntry(),
			playlistEntry([track(1, { downloadable: true }), ...stubs, {}])
		]);

		const { calls } = mockFetch((url) => {
			expect(url.pathname).toBe('/tracks');
			const ids = url.searchParams.get('ids')!.split(',').map(Number);

			return jsonResponse([
				...ids
					.filter((id) => id <= 3)
					.map((id) => track(id, { downloadable: id === 2, purchaseUrl: id === 3 ? 'https://buy.test/3' : null })),
				{ kind: 'playlist', id: 999, permalink_url: 'https://soundcloud.com/x/sets/y' },
				null
			]);
		});

		await loadContentScript();
		// The full track was harvested on load.
		await vi.advanceTimersByTimeAsync(1000);
		expect(storedDetails()[permalink(1)]).toMatchObject({ downloadable: true });

		const response = await getDetails();
		expect(calls).toHaveLength(1);
		expect(calls[0].url.origin).toBe('https://api-v2.soundcloud.com');
		expect(calls[0].url.searchParams.get('ids')).toBe(Array.from({ length: 50 }, (_, i) => i + 2).join(','));
		expect(calls[0].url.searchParams.get('client_id')).toBe(CLIENT_ID);
		expect(calls[0].init).toEqual({ credentials: 'omit' });
		const details = detailsOf(response);
		expect(details[permalink(1)]).toMatchObject({ downloadable: true });
		expect(details[permalink(2)]).toMatchObject({ downloadable: true, purchaseUrl: '' });
		expect(details[permalink(3)]).toMatchObject({ downloadable: false, purchaseUrl: 'https://buy.test/3' });
		expect(response).toMatchObject({ lookupFailed: 0, resumed: false, error: '' });
	});

	it('skips stubs already known, goes through every batch, then resolves what is left one by one', async () => {
		const known = {
			id: 5,
			permalink: '/a5/t5',
			downloadable: false,
			hasDownloadsLeft: true,
			purchaseUrl: '',
			purchaseTitle: '',
			genre: '',
			artworkUrl: '',
			at: NOW - DAY
		};

		globalThis.chrome = createChrome({ storage: { details: { '/a5/t5': known } } });
		render(
			playlistPage({
				rows: [
					row(1),
					row(5),
					row(60),
					playlistRow({ artist: 'P', title: 'Private', href: '/p/private?in=curator/sets/my-playlist' })
				]
			}),
			SET
		);
		const stubs = Array.from({ length: 60 }, (_, i) => ({ id: i + 1 }));
		hydrationScript([apiClientEntry(), playlistEntry(stubs)]);

		const { paths } = mockFetch((url) => {
			if (url.pathname === '/tracks') {
				const ids = url.searchParams.get('ids')!.split(',').map(Number);

				return jsonResponse(ids.filter((id) => [1, 60].includes(id)).map((id) => track(id)));
			}

			return jsonResponse({ kind: 'user', id: 1 }); // /resolve of the private track: not a track
		});

		await loadContentScript();
		const response = await getDetails();
		const ids = (p: string) => new URL(`https://x${p}`).searchParams.get('ids')!.split(',').map(Number);
		expect(paths()[0]).toMatch(/^\/tracks\?/);
		expect(ids(paths()[0])).toEqual(
			Array.from({ length: 59 }, (_, i) => i + 1)
				.filter((id) => id !== 5)
				.slice(0, 50)
		);
		expect(ids(paths()[1])).toEqual([52, 53, 54, 55, 56, 57, 58, 59, 60]);
		expect(paths()[2]).toMatch(/^\/resolve\?url=https%3A%2F%2Fsoundcloud\.com%2Fp%2Fprivate/);
		expect(paths()).toHaveLength(3);
		expect(detailsOf(response)['/p/private']).toBeNull();
		expect(response.lookupFailed).toBe(1);
	});

	it('asks API_CONCURRENCY batches at once when the whole playlist is rendered', async () => {
		// Two hundred rendered rows reach four batches down the playlist, so the
		// lookup says so up front and goes as wide as it is allowed. Discovering
		// that a doubling at a time, as it used to, cost a scrolled page a whole
		// extra round trip — half a second to two seconds of the wait.
		render(playlistPage({ rows: Array.from({ length: 200 }, (_, i) => row(i + 1)) }), SET);
		hydrationScript([apiClientEntry(), playlistEntry(Array.from({ length: 500 }, (_, i) => ({ id: i + 1 })))]);
		const inFlight: (() => void)[] = [];

		const { calls } = mockFetch(
			(url) =>
				new Promise((resolve) => {
					const ids = url.searchParams.get('ids')!.split(',').map(Number);
					inFlight.push(() => resolve(jsonResponse(ids.map((id) => track(id)))));
				})
		);

		await loadContentScript();
		const pending = getDetails();
		await flush();
		expect(calls).toHaveLength(API_CONCURRENCY);

		while (inFlight.length) inFlight.shift()!();
		await flush();
		const response = await pending;
		// Four batches of fifty cover all two hundred rows, and nothing beyond
		// them was asked for: the other six batches of the playlist are untouched.
		expect(calls).toHaveLength(4);
		expect(response.tracks.every((t) => t.details)).toBe(true);
		expect(response.lookupFailed).toBe(0);
	});

	it('asks one batch at a time when only the top of a long playlist is rendered', async () => {
		// Three rows reach one batch down, so one request goes out — a page nobody
		// has scrolled costs SoundCloud exactly what it did before the concurrency.
		render(playlistPage({ rows: [row(1), row(2), row(3)] }), SET);
		hydrationScript([apiClientEntry(), playlistEntry(Array.from({ length: 500 }, (_, i) => ({ id: i + 1 })))]);
		const inFlight: (() => void)[] = [];

		const { calls } = mockFetch(
			(url) =>
				new Promise((resolve) => {
					const ids = url.searchParams.get('ids')!.split(',').map(Number);
					inFlight.push(() => resolve(jsonResponse(ids.map((id) => track(id)))));
				})
		);

		await loadContentScript();
		const pending = getDetails();
		await flush();
		expect(calls).toHaveLength(1);

		while (inFlight.length) inFlight.shift()!();
		await flush();
		await pending;
		expect(calls).toHaveLength(1);
	});

	it('accepts a private playlist’s tokenised path as the hydrated playlist', async () => {
		render(playlistPage({ rows: [row(1)] }), `${SET}/s-Token1`);
		document.querySelector('.trackItem__trackTitle')!.setAttribute('href', '/a1/t1');
		hydrationScript([apiClientEntry(), playlistEntry([{ id: 1 }])]);
		const { paths } = mockFetch(() => jsonResponse([track(1)]));
		await loadContentScript();
		const response = await getDetails();
		expect(paths()[0]).toMatch(/^\/tracks\?ids=1/);
		expect(detailsOf(response)[permalink(1)]).toMatchObject({ id: 1 });
	});

	it('resolves the playlist first when the hydration describes another page', async () => {
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry(), playlistEntry([{ id: 1 }], '/someone/sets/previous-page')]);

		const { paths } = mockFetch((url) => {
			if (url.pathname === '/resolve') return jsonResponse(playlistEntry([{ id: 1 }]).data);

			return jsonResponse([track(1)]);
		});

		await loadContentScript();
		const response = await getDetails();
		expect(paths()[0]).toBe(
			`/resolve?url=${encodeURIComponent(`https://soundcloud.com${SET}`)}&client_id=${CLIENT_ID}`
		);
		expect(paths()[1]).toMatch(/^\/tracks\?ids=1&/);
		expect(detailsOf(response)[permalink(1)]).toMatchObject({ id: 1 });
	});

	// The playlist's ids do not change while the page is open, so a second panel
	// opened on it — the usual thing after scrolling — should spend its requests
	// on the rows it came for and none on asking what playlist this is again.
	it('resolves the playlist once however many lookups the page sees', async () => {
		// Sixty tracks, so the stubs need two batches and the first lookup — whose
		// one rendered row is in the first batch — stops before the second.
		const stubs = Array.from({ length: 60 }, (_, i) => ({ id: i + 1 }));
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry(), playlistEntry([{ id: 1 }], '/someone/sets/previous-page')]);

		const { paths } = mockFetch((url) => {
			if (url.pathname === '/resolve') return jsonResponse(playlistEntry(stubs).data);

			return jsonResponse(
				url.searchParams
					.get('ids')!
					.split(',')
					.map((id) => track(Number(id)))
			);
		});

		await loadContentScript();
		await getDetails();
		expect(paths()).toEqual([
			`/resolve?url=${encodeURIComponent(`https://soundcloud.com${SET}`)}&client_id=${CLIENT_ID}`,
			expect.stringMatching(/^\/tracks\?ids=1%2C/)
		]);

		// The user scrolls; a row from the second batch renders and the panel is
		// opened again. That lookup needs the playlist's ids too — and gets them
		// without asking SoundCloud what playlist this is a second time.
		document.querySelector('.trackList__list')!.insertAdjacentHTML('beforeend', row(55));
		await getDetails();
		expect(paths().filter((p) => p.startsWith('/resolve'))).toHaveLength(1);
		expect(paths()[2]).toMatch(/^\/tracks\?ids=51%2C/);
	});

	it('resolves the playlist again after a resolve that failed', async () => {
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry(), playlistEntry([{ id: 1 }], '/someone/sets/previous-page')]);

		const { paths } = mockFetch((url, _init, n) => {
			if (url.pathname !== '/resolve') return jsonResponse([track(1)]);

			return n === 1 ? jsonResponse({}, 500) : jsonResponse(playlistEntry([{ id: 1 }]).data);
		});

		await loadContentScript();
		await getDetails();
		document.querySelector('.trackList__list')!.insertAdjacentHTML('beforeend', row(2));
		await getDetails();
		expect(
			paths().filter((p) => p.startsWith(`/resolve?url=${encodeURIComponent('https://soundcloud.com' + SET)}`))
		).toHaveLength(2);
	});

	it('falls back to resolving rows individually when the playlist cannot be resolved', async () => {
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry()]);

		const { paths } = mockFetch((url) => {
			const target = url.searchParams.get('url')!;

			if (target.endsWith('/sets/my-playlist')) return jsonResponse({ kind: 'user', id: 3 });

			return jsonResponse(track(1));
		});

		await loadContentScript();
		const response = await getDetails();
		expect(paths().map((p) => decodeURIComponent(p.split('&')[0]))).toEqual([
			`/resolve?url=https://soundcloud.com${SET}`,
			`/resolve?url=https://soundcloud.com${permalink(1)}`
		]);
		expect(detailsOf(response)[permalink(1)]).toMatchObject({ id: 1 });
	});

	it('falls back to resolving rows individually when the playlist request fails', async () => {
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry()]);

		const { calls } = mockFetch((url) => {
			if (url.searchParams.get('url')!.includes('/sets/')) return jsonResponse({}, 500);

			return jsonResponse(track(1));
		});

		await loadContentScript();
		const response = await getDetails();
		expect(calls).toHaveLength(2);
		expect(detailsOf(response)[permalink(1)]).toMatchObject({ id: 1 });
	});
});

describe('likes pages', () => {
	const likedRow = (n: number, extra: LikedSoundOptions = {}) =>
		likedSound({ title: `L${n}`, href: `/l${n}/t${n}`, ...extra });

	const liked = (n: number) => `/l${n}/t${n}`;

	const likedTrack = (n: number, extra: ApiTrackOptions = {}) =>
		apiTrack({ id: 100 + n, permalink: liked(n), ...extra });

	it('reads the signed-in user’s likes listing, sized to the rendered rows, page by page', async () => {
		render(
			ownLikesPage({
				list: true,
				items: [likedRow(0), likedSound({ playlist: true, href: '/x/sets/p' }), likedRow(2), likedRow(3)]
			}),
			'/you/likes'
		);
		hydrationScript([apiClientEntry(), { hydratable: 'meUser', data: { id: 42 } }]);

		const { calls } = mockFetch((_url, _init, n) => {
			if (n === 1) {
				return jsonResponse(
					likesListing(
						[likedTrack(0), null, { kind: 'playlist', id: 7, permalink_url: 'https://soundcloud.com/x/sets/p' }],
						'https://api-v2.soundcloud.com/users/42/likes?offset=3&limit=9'
					)
				);
			}

			return jsonResponse(likesListing([likedTrack(2), likedTrack(3)]));
		});

		await loadContentScript();
		const response = await getDetails();
		expect(calls).toHaveLength(2);
		expect(calls[0].url.pathname).toBe('/users/42/likes');
		expect(Object.fromEntries(calls[0].url.searchParams)).toEqual({
			limit: '9',
			offset: '0',
			linked_partitioning: '1',
			client_id: CLIENT_ID
		});
		expect(Object.fromEntries(calls[1].url.searchParams)).toEqual({ offset: '3', limit: '200', client_id: CLIENT_ID });
		expect(response.tracks.map((t) => t.details!.id)).toEqual([100, 102, 103]);
		expect(response.skippedPlaylists).toBe(1);
	});

	it('stops following the listing when it ends, and resolves the rest individually', async () => {
		render(publicLikesPage({ user: 'dj', items: [likedRow(1), likedRow(2)] }), '/dj/likes');
		hydrationScript([apiClientEntry(), { hydratable: 'user', data: { id: 7, permalink: 'dj' } }]);

		const { paths } = mockFetch((url) => {
			if (url.pathname === '/users/7/likes') return jsonResponse({ collection: [{ track: likedTrack(1) }] });

			return jsonResponse(likedTrack(2));
		});

		await loadContentScript();
		const response = await getDetails();
		expect(paths().map((p) => p.split('?')[0])).toEqual(['/users/7/likes', '/resolve']);
		expect(response.tracks.map((t) => t.details!.id)).toEqual([101, 102]);
	});

	it('gives up on the listing after ten pages', async () => {
		render(publicLikesPage({ user: 'dj', items: [likedRow(1), likedRow(2)] }), '/dj/likes');
		hydrationScript([apiClientEntry(), { hydratable: 'user', data: { id: 7, permalink: 'dj' } }]);

		const { paths } = mockFetch((url) => {
			if (url.pathname === '/users/7/likes')
				return jsonResponse(
					likesListing(
						[apiTrack({ id: 1, permalink: '/unrelated/like' })],
						'https://api-v2.soundcloud.com/users/7/likes?offset=1'
					)
				);

			return jsonResponse(likedTrack(Number(url.searchParams.get('url')!.slice(-1))));
		});

		await loadContentScript();
		await getDetails();
		expect(paths().filter((p) => p.startsWith('/users/7/likes'))).toHaveLength(10);
		expect(paths().filter((p) => p.startsWith('/resolve'))).toHaveLength(2);
	});

	it('copes with a listing that answers nothing', async () => {
		render(publicLikesPage({ user: 'dj', items: [likedRow(1), likedRow(2)] }), '/dj/likes');
		hydrationScript([apiClientEntry(), { hydratable: 'user', data: { id: 7, permalink: 'dj' } }]);

		const { paths } = mockFetch((url) => {
			if (url.pathname === '/users/7/likes') return jsonResponse(null);

			return jsonResponse(likedTrack(Number(url.searchParams.get('url')!.slice(-1))));
		});

		await loadContentScript();
		const response = await getDetails();
		expect(paths().map((p) => p.split('?')[0])).toEqual(['/users/7/likes', '/resolve', '/resolve']);
		expect(response.tracks.map((t) => t.details!.id)).toEqual([101, 102]);
	});

	it('resolves the profile first when the hydration holds another user, and skips the listing for an unknown one', async () => {
		render(publicLikesPage({ user: 'dj', items: [likedRow(1), likedRow(2)] }), '/dj/likes');
		hydrationScript([apiClientEntry(), { hydratable: 'user', data: { id: 8, permalink: 'someone-else' } }]);

		const { paths } = mockFetch((url) => {
			const target = url.searchParams.get('url');

			if (target === 'https://soundcloud.com/dj') return jsonResponse({ kind: 'user', id: 7 });

			if (url.pathname === '/users/7/likes') return jsonResponse(likesListing([likedTrack(1), likedTrack(2)]));

			return undefined;
		});

		await loadContentScript();
		const response = await getDetails();
		expect(paths().map((p) => decodeURIComponent(p.split('&')[0]))).toEqual([
			'/resolve?url=https://soundcloud.com/dj',
			'/users/7/likes?limit=7'
		]);
		expect(response.tracks.map((t) => t.details!.id)).toEqual([101, 102]);
	});

	it('resolves rows one by one when the profile does not resolve to a user', async () => {
		render(publicLikesPage({ user: 'gone', items: [likedRow(1), likedRow(2)] }), '/gone/likes');
		hydrationScript([apiClientEntry()]);

		const second = mockFetch((url) => {
			const target = url.searchParams.get('url')!;

			if (target === 'https://soundcloud.com/gone') return jsonResponse({ kind: 'playlist', id: 7 });

			return jsonResponse(likedTrack(Number(target.slice(-1))));
		});

		await loadContentScript();
		await getDetails();
		expect(second.paths().map((p) => decodeURIComponent(p.split('&')[0]))).toEqual([
			'/resolve?url=https://soundcloud.com/gone',
			`/resolve?url=https://soundcloud.com${liked(1)}`,
			`/resolve?url=https://soundcloud.com${liked(2)}`
		]);
	});

	// Scrolling a Likes page and opening the panel again is the ordinary way to
	// use it, and every opening used to walk the listing from the top: ten
	// scrolls of a two-thousand-like page cost fifty-five requests for ten pages
	// of data. Carrying the cursor makes the second walk ask only for what the
	// first one had not reached.
	it('carries on the listing from where the last walk stopped', async () => {
		const rows = (from: number, to: number) => Array.from({ length: to - from }, (_, i) => likedRow(from + i));
		render(publicLikesPage({ user: 'dj', items: rows(0, 3) }), '/dj/likes');
		hydrationScript([apiClientEntry(), { hydratable: 'user', data: { id: 7, permalink: 'dj' } }]);

		const page = (from: number, to: number, next: boolean) =>
			jsonResponse(
				likesListing(
					Array.from({ length: to - from }, (_, i) => likedTrack(from + i)),
					next ? `https://api-v2.soundcloud.com/users/7/likes?offset=${to}&limit=200` : null
				)
			);

		const { calls, paths } = mockFetch((url) => {
			const offset = Number(url.searchParams.get('offset') || 0);

			return page(offset, offset + 3, true);
		});

		await loadContentScript();
		await getDetails();
		expect(paths()).toHaveLength(1);

		// The user scrolls: three more rows render, all of them past what the
		// first walk covered.
		document.querySelector('.soundList')!.insertAdjacentHTML('beforeend', rows(3, 6).join(''));
		await getDetails();
		expect(paths()).toHaveLength(2);
		// The second walk resumed rather than starting over.
		expect(calls[1].url.searchParams.get('offset')).toBe('3');
	});

	it('walks the listing from the top again when a wanted row is above the cursor', async () => {
		const rows = (from: number, to: number) => Array.from({ length: to - from }, (_, i) => likedRow(from + i));
		render(publicLikesPage({ user: 'dj', items: rows(0, 3) }), '/dj/likes');
		hydrationScript([apiClientEntry(), { hydratable: 'user', data: { id: 7, permalink: 'dj' } }]);

		const { calls } = mockFetch((url) => {
			if (url.pathname !== '/users/7/likes') return jsonResponse(likedTrack(99));
			const offset = Number(url.searchParams.get('offset') || 0);

			return jsonResponse(
				likesListing(
					Array.from({ length: 3 }, (_, i) => likedTrack(offset + i)),
					`https://api-v2.soundcloud.com/users/7/likes?offset=${offset + 3}&limit=200`
				)
			);
		});

		await loadContentScript();
		await getDetails();

		// Likes added at the top since: the new rows are items 0-2, above the
		// cursor, so the listing is walked from the top rather than resumed.
		document
			.querySelector('.soundList')!
			.insertAdjacentHTML('afterbegin', [likedRow(97), likedRow(98), likedRow(99)].join(''));
		await getDetails();
		expect(calls[1].url.pathname).toBe('/users/7/likes');
		expect(calls[1].url.searchParams.get('offset')).toBe('0');
	});

	it('drops a cursor the listing no longer honours', async () => {
		const rows = (from: number, to: number) => Array.from({ length: to - from }, (_, i) => likedRow(from + i));
		render(publicLikesPage({ user: 'dj', items: rows(0, 3) }), '/dj/likes');
		hydrationScript([apiClientEntry(), { hydratable: 'user', data: { id: 7, permalink: 'dj' } }]);
		// The cursor goes stale once — the walk that follows must not keep using it.
		let expired = false;

		const { calls } = mockFetch((url) => {
			if (url.pathname !== '/users/7/likes')
				return jsonResponse(likedTrack(Number(/t(\d+)$/.exec(url.searchParams.get('url')!)![1])));
			const offset = Number(url.searchParams.get('offset') || 0);

			if (offset === 3 && !expired) {
				expired = true;

				return jsonResponse({}, 404);
			}

			return jsonResponse(
				likesListing(
					Array.from({ length: 3 }, (_, i) => likedTrack(offset + i)),
					`https://api-v2.soundcloud.com/users/7/likes?offset=${offset + 3}&limit=200`
				)
			);
		});

		await loadContentScript();
		await getDetails();
		document.querySelector('.soundList')!.insertAdjacentHTML('beforeend', rows(3, 9).join(''));
		// The stale cursor sends this one to the per-row fallback …
		await getDetails();
		expect(calls.filter((c) => c.url.pathname === '/resolve')).not.toHaveLength(0);

		// … and the next walk starts from the top rather than from the dead cursor.
		const before = calls.length;
		document.querySelector('.soundList')!.insertAdjacentHTML('beforeend', rows(9, 15).join(''));
		await getDetails();
		const listing = calls.slice(before).find((c) => c.url.pathname === '/users/7/likes');
		expect(listing!.url.searchParams.get('offset')).toBe('0');
	});

	it('skips the listing when signed out on /you/likes', async () => {
		render(
			ownLikesPage({
				items: [likedTile({ title: 'One', href: liked(1) }), likedTile({ title: 'Two', href: liked(2) })]
			}),
			'/you/likes'
		);
		hydrationScript([apiClientEntry()]);
		const first = mockFetch((url) => jsonResponse(likedTrack(Number(url.searchParams.get('url')!.slice(-1)))));
		await loadContentScript();
		await getDetails();
		expect(first.paths().map((p) => p.split('?')[0])).toEqual(['/resolve', '/resolve']);
	});

	it('skips the listing when resolving directly is cheaper', async () => {
		render(ownLikesPage({ items: [likedTile({ title: 'One', href: liked(1) })] }), '/you/likes');
		hydrationScript([apiClientEntry(), { hydratable: 'meUser', data: { id: 42 } }]);
		const second = mockFetch(() => jsonResponse(likedTrack(1)));
		await loadContentScript();
		await getDetails();
		expect(second.paths().map((p) => p.split('?')[0])).toEqual(['/resolve']);
	});

	it('falls back to resolving rows when the listing request fails', async () => {
		render(publicLikesPage({ user: 'dj', items: [likedRow(1), likedRow(2)] }), '/dj/likes');
		hydrationScript([apiClientEntry(), { hydratable: 'user', data: { id: 7, permalink: 'dj' } }]);

		const { paths } = mockFetch((url) => {
			if (url.pathname === '/users/7/likes') return jsonResponse({}, 503);

			return jsonResponse(likedTrack(Number(url.searchParams.get('url')!.slice(-1))));
		});

		await loadContentScript();
		const response = await getDetails();
		expect(paths().map((p) => p.split('?')[0])).toEqual(['/users/7/likes', '/resolve', '/resolve']);
		expect(response.tracks.every((t) => t.details)).toBe(true);
	});

	it('uses the purchase link the page shows when a row cannot be looked up', async () => {
		render(
			publicLikesPage({
				user: 'dj',
				items: [
					likedRow(1, { purchase: { url: 'https://hypeddit.com/one', label: 'Free Download' } }),
					likedRow(2, { purchase: { url: 'https://bandcamp.com/two', label: '' } }),
					likedRow(3)
				]
			}),
			'/dj/likes'
		);
		hydrationScript([apiClientEntry()]);
		mockFetch(() => jsonResponse({}, 404));
		await loadContentScript();
		const response = await getDetails();
		expect(response.tracks.map((t) => t.details)).toEqual([
			{
				id: null,
				permalink: liked(1),
				downloadable: null,
				hasDownloadsLeft: null,
				purchaseUrl: 'https://hypeddit.com/one',
				purchaseTitle: 'Free Download',
				genre: null,
				artworkUrl: '',
				partial: true
			},
			{
				id: null,
				permalink: liked(2),
				downloadable: null,
				hasDownloadsLeft: null,
				purchaseUrl: 'https://bandcamp.com/two',
				purchaseTitle: '',
				genre: null,
				artworkUrl: '',
				partial: true
			},
			null
		]);
		expect(response.lookupFailed).toBe(3);
	});
});

describe('resolving tracks one at a time', () => {
	it('keys the answer by the permalink asked for, API_CONCURRENCY requests at a time', async () => {
		render(playlistPage({ rows: [row(1), row(2), row(3), row(4), row(5), row(6), row(1)] }), SET);
		hydrationScript([apiClientEntry()]);
		const inFlight: (() => void)[] = [];

		const { calls } = mockFetch(
			notThePlaylist(
				(url) =>
					new Promise((resolve) => {
						const n = trackNumber(url);
						inFlight.push(() => resolve(jsonResponse(track(n, { permalink: `/renamed/t${n}` }))));
					})
			)
		);

		await loadContentScript();
		const pending = getDetails();
		await flush();
		// The playlist itself, then four rows and no fifth: six are wanted, so
		// what is in the air is the concurrency rather than the whole list.
		expect(calls).toHaveLength(1 + API_CONCURRENCY);
		inFlight.shift()!();
		await flush();
		expect(calls).toHaveLength(1 + API_CONCURRENCY + 1); // the freed worker took the next

		while (inFlight.length) inFlight.shift()!();
		await flush();

		while (inFlight.length) inFlight.shift()!();
		const response = await pending;
		expect(calls).toHaveLength(1 + 6); // six unique rows: the duplicate was asked for once
		const details = detailsOf(response);

		for (const n of [1, 2, 3, 4, 5, 6]) expect(details[permalink(n)]).toMatchObject({ id: n, permalink: permalink(n) });
	});

	it('remembers tracks that resolve to something else, or not at all, as failures for a day', async () => {
		render(playlistPage({ rows: [row(1), row(2)] }), SET);
		hydrationScript([apiClientEntry()]);
		mockFetch(
			notThePlaylist((url) =>
				url.searchParams.get('url')!.endsWith('t1') ? jsonResponse({ kind: 'user' }) : jsonResponse({}, 404)
			)
		);
		await loadContentScript();
		const response = await getDetails();
		expect(response.tracks.map((t) => t.details)).toEqual([null, null]);
		expect(response.lookupFailed).toBe(2);
		await flush();
		expect(storedDetails()[permalink(1)]).toEqual({ unavailable: true, at: NOW });
	});

	it('stops at a 401 and reports the key as rejected, without remembering a failure', async () => {
		render(playlistPage({ rows: [row(1), row(2), row(3), row(4), row(5), row(6)] }), SET);
		hydrationScript([apiClientEntry()]);
		const { calls } = mockFetch(() => jsonResponse({}, 401));
		await loadContentScript();
		const response = await getDetails();
		expect(response.error).toBe('unauthorized');
		// Only what was already in the air: no worker picked up a row after the
		// first 401 came back.
		expect(calls.filter((c) => !isPlaylistResolve(c.url))).toHaveLength(API_CONCURRENCY);
		expect(response.tracks.map((t) => t.details)).toEqual([null, null, null, null, null, null]);
		expect(response.lookupFailed).toBe(0);
		await flush();
		expect(storedDetails()).toBeUndefined();
	});

	it('backs off and retries a rate-limited request, then gives up', async () => {
		render(playlistPage({ rows: [row(1), row(2)] }), SET);
		hydrationScript([apiClientEntry()]);
		const attempts = { 1: 0, 2: 0 };

		const { calls } = mockFetch(
			notThePlaylist((url) => {
				const n = trackNumber(url);

				if (n !== 1 && n !== 2) throw new Error(`no track ${n} on this page`);
				attempts[n]++;

				if (n === 1) return attempts[n] === 1 ? jsonResponse({}, 429) : jsonResponse(track(1));

				return jsonResponse({}, 429);
			})
		);

		await loadContentScript();
		const pending = getDetails();
		await flush();
		expect(attempts).toEqual({ 1: 1, 2: 1 });
		await vi.advanceTimersByTimeAsync(1500);
		expect(attempts).toEqual({ 1: 2, 2: 2 });
		await vi.advanceTimersByTimeAsync(3000);
		const response = await pending;
		expect(attempts).toEqual({ 1: 2, 2: 3 });
		expect(calls).toHaveLength(1 + 5);
		expect(detailsOf(response)[permalink(1)]).toMatchObject({ id: 1 });
		expect(detailsOf(response)[permalink(2)]).toBeNull();
		expect(response.lookupFailed).toBe(1);
	});
});

describe('a page the anonymous API cannot see', () => {
	// The requests are made the way a signed-out visitor would make them, so a
	// private playlist is one SoundCloud will not describe however it is asked.
	// What used to happen then was the worst of both: the bulk request came back
	// empty and every row was asked for in turn, one round trip each, all of
	// them failing. Ninety tracks was a minute of "still checking" and an answer
	// of no.
	const PROBE = 5; // content.js: rows resolved before giving the rest up

	const blind = (rows: number[]) => {
		render(playlistPage({ rows: rows.map(row) }), SET);
		hydrationScript([apiClientEntry(), playlistEntry(rows.map((n) => ({ id: n })))]);
	};

	it('gives the rest up once a handful of rows have come back with nothing', async () => {
		const rows = Array.from({ length: 40 }, (_, i) => i + 1);
		blind(rows);
		// Everything is answerable and answers with nothing, as SoundCloud does
		// for a track a signed-out visitor may not see.
		const { paths } = mockFetch(() => jsonResponse([]));
		await loadContentScript();
		const response = await getDetails();

		const batches = paths().filter((p) => p.startsWith('/tracks?'));
		const resolves = paths().filter((p) => p.startsWith('/resolve'));
		expect(batches).toHaveLength(1);
		// Five asked for, not forty: the other thirty-five round trips are the
		// whole point of this.
		expect(resolves).toHaveLength(PROBE);
		expect(response.unseen).toBe(true);
		expect(response.tracks.every((t) => t.details === null)).toBe(true);
	});

	it('carries on through the rest when one of the handful does come back', async () => {
		const rows = Array.from({ length: 40 }, (_, i) => i + 1);
		blind(rows);

		// The bulk request covers nothing, but the rows themselves resolve — a
		// visible page whose bulk source simply did not reach them.
		const { paths } = mockFetch((url) =>
			url.pathname === '/tracks' ? jsonResponse([]) : jsonResponse(track(trackNumber(url)))
		);

		await loadContentScript();
		const response = await getDetails();

		expect(paths().filter((p) => p.startsWith('/resolve'))).toHaveLength(40);
		expect(response.unseen).toBe(false);
		expect(response.tracks.every((t) => t.details)).toBe(true);
	});

	it('asks about every row when there are only a handful of them', async () => {
		// Below the probe there is nothing to save, and stopping early would only
		// make a short page answer worse than a long one.
		blind([1, 2, 3]);
		const { paths } = mockFetch(() => jsonResponse([]));
		await loadContentScript();
		const response = await getDetails();
		expect(paths().filter((p) => p.startsWith('/resolve'))).toHaveLength(3);
		expect(response.unseen).toBe(false);
	});
});

describe('one lookup at a time', () => {
	it('lets a second request for the same tracks join the running lookup', async () => {
		render(playlistPage({ rows: [row(1), row(2)] }), SET);
		hydrationScript([apiClientEntry()]);
		const answers: (() => void)[] = [];

		const { calls } = mockFetch(
			notThePlaylist(
				(url) =>
					new Promise((resolve) => {
						answers.push(() => resolve(jsonResponse(track(trackNumber(url)))));
					})
			)
		);

		chrome.runtime.sendMessage.mockResolvedValue(undefined);
		await loadContentScript();
		const first = getDetails();
		await flush();
		const second = getDetails();
		await flush();
		expect(calls).toHaveLength(1 + 2);
		expect(chrome.runtime.sendMessage).toHaveBeenLastCalledWith({
			type: 'DOWNLOAD_DETAILS_PROGRESS',
			done: 0,
			total: 2,
			resumed: true,
			details: []
		});
		answers.shift()!();
		await flush();
		// The answer that has just arrived rides along with the count, so the
		// panel can fill that row in rather than wait for the other one.
		expect(chrome.runtime.sendMessage).toHaveBeenLastCalledWith({
			type: 'DOWNLOAD_DETAILS_PROGRESS',
			done: 1,
			total: 2,
			resumed: true,
			details: [expect.objectContaining({ id: 1, permalink: permalink(1) })]
		});
		answers.shift()!();
		const [a, b] = await Promise.all([first, second]);
		expect(a.resumed).toBe(false);
		expect(b.resumed).toBe(true);
		expect(calls).toHaveLength(1 + 2);
		expect(detailsOf(b)).toEqual(detailsOf(a));
	});

	it('runs a request for other tracks after the lookup in progress', async () => {
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry()]);
		const answers: (() => void)[] = [];
		const rowCalls = () => calls.filter((c) => !isPlaylistResolve(c.url));

		const { calls } = mockFetch(
			notThePlaylist(
				(url) =>
					new Promise((resolve) => {
						answers.push(() => resolve(jsonResponse(track(trackNumber(url)))));
					})
			)
		);

		await loadContentScript();
		const first = getDetails();
		await flush();
		// The user navigates in-app to another playlist and opens the panel again.
		document.querySelector('.trackItem__trackTitle')!.setAttribute('href', '/a2/t2?in=curator/sets/other');
		const second = getDetails();
		await flush();
		expect(rowCalls()).toHaveLength(1);
		answers.shift()!();
		await flush();
		expect(rowCalls()).toHaveLength(2);
		answers.shift()!();
		const [a, b] = await Promise.all([first, second]);
		expect(detailsOf(a)[permalink(1)]).toMatchObject({ id: 1 });
		expect(detailsOf(b)[permalink(2)]).toMatchObject({ id: 2 });
		expect(b.resumed).toBe(false);
	});

	it('answers a request left to a lookup that ends without settling it', async () => {
		render(playlistPage({ rows: [row(1), row(2)] }), SET);
		hydrationScript([apiClientEntry()]);
		mockFetch(() => jsonResponse({}, 401));
		await loadContentScript();
		const first = getDetails();
		const second = getDetails();
		const [a, b] = await Promise.all([first, second]);
		expect(a.error).toBe('unauthorized');
		expect(b.error).toBe('unauthorized');
		expect(b.resumed).toBe(true);
	});

	it('waits for the marks and cache to load before looking anything up', async () => {
		globalThis.chrome = createChrome({
			storage: {
				details: {
					[permalink(1)]: {
						id: 1,
						permalink: permalink(1),
						downloadable: true,
						hasDownloadsLeft: true,
						purchaseUrl: '',
						purchaseTitle: '',
						genre: '',
						artworkUrl: '',
						at: NOW
					}
				}
			}
		});
		let release!: () => void;
		const gate = new Promise<void>((resolve) => (release = resolve));
		const realGet = chrome.storage.local.get.getMockImplementation()!;
		chrome.storage.local.get.mockImplementation(async (keys) => {
			if (keys === 'details') await gate;

			return realGet(keys);
		});
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry()]);
		const { fn } = mockFetch(() => jsonResponse(track(1)));
		await loadContentScript();
		let response: DetailsAnswer | null = null;
		void getDetails().then((r) => (response = r));
		await flush();
		expect(response).toBeNull();
		release();
		await flush();
		expect(response).not.toBeNull();
		expect(fn).not.toHaveBeenCalled();
		expect(response!.tracks[0].details).toMatchObject({ downloadable: true });
	});

	it('still looks tracks up when the extension’s storage is unavailable', async () => {
		chrome.storage.local.get.mockRejectedValue(new Error('Extension context invalidated'));
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry()]);
		mockFetch(notThePlaylist(() => jsonResponse(track(1))));
		await loadContentScript();
		const response = await getDetails();
		expect(response.ok).toBe(true);
		expect(detailsOf(response)[permalink(1)]).toMatchObject({ id: 1 });
	});
});

describe('when reporting progress fails', () => {
	it('answers with the failure when the panel cannot be reached at all', async () => {
		render(playlistPage({ rows: [row(1)] }), SET);
		hydrationScript([apiClientEntry()]);
		mockFetch(notThePlaylist(() => jsonResponse(track(1))));
		await loadContentScript();
		chrome.runtime.sendMessage.mockImplementation(() => {
			throw new Error('Extension context invalidated.');
		});
		expect(await askDetails()).toEqual({ ok: false, error: 'Extension context invalidated.' });

		chrome.runtime.sendMessage.mockImplementation(() => {
			throw 'gone';
		});
		expect(await askDetails()).toEqual({ ok: false, error: 'gone' });
	});

	it('keeps what a lookup learned before it blew up', async () => {
		render(playlistPage({ rows: [row(1), row(2), row(3)] }), SET);
		hydrationScript([apiClientEntry()]);
		const answers: (() => void)[] = [];
		mockFetch(
			notThePlaylist(
				(url) =>
					new Promise((resolve) => {
						answers.push(() => resolve(jsonResponse(track(trackNumber(url)))));
					})
			)
		);
		await loadContentScript();
		const pending = getDetails();
		await flush();
		chrome.runtime.sendMessage.mockImplementation(() => {
			throw new Error('Extension context invalidated.');
		});
		answers.shift()!();
		const response = await pending;
		expect(response.ok).toBe(true);
		expect(response.tracks).toHaveLength(3);
		// The answer that arrived is the answer, not a failure: it was telling the
		// panel about it that went wrong, not asking SoundCloud. The rest were
		// never answered, and are not failures either.
		expect(detailsOf(response)[permalink(1)]).toMatchObject({ id: 1, permalink: permalink(1) });
		expect(detailsOf(response)[permalink(2)]).toBeNull();
		expect(response.lookupFailed).toBe(0);
	});
});

describe('cover art a row never loaded', () => {
	// A page lazy-loads its thumbnails as they scroll into view, so most rows of
	// a list read without scrolling through it carry no artwork URL at all. The
	// answer SoundCloud gives about each track names one, at no extra request.
	const ART_URL = 'https://i1.sndcdn.com/artworks-hash1-large.jpg';
	const AVATAR_URL = 'https://i1.sndcdn.com/avatars-hash2-large.jpg';

	const artworkOf = (response: DetailsAnswer) =>
		Object.fromEntries(response.tracks.map((t) => [t.permalink, t.artworkUrl]));

	it('takes the cover from the looked-up details', async () => {
		render(playlistPage({ rows: [playlistRow({ artist: 'A1', title: 'T1', href: permalink(1), artwork: '' })] }), SET);
		hydrationScript([apiClientEntry(), playlistEntry([track(1, { artworkUrl: ART_URL })])]);
		mockFetch();
		await loadContentScript();

		const response = await getDetails();
		expect(response.tracks[0].details!.artworkUrl).toBe(ART_URL);
		expect(artworkOf(response)).toEqual({ [permalink(1)]: ART_URL });
	});

	it('falls back to the uploader’s avatar, as the page’s own rows do', async () => {
		render(playlistPage({ rows: [playlistRow({ artist: 'A1', title: 'T1', href: permalink(1), artwork: '' })] }), SET);
		hydrationScript([apiClientEntry(), playlistEntry([track(1, { avatarUrl: AVATAR_URL })])]);
		mockFetch();
		await loadContentScript();

		expect(artworkOf(await getDetails())).toEqual({ [permalink(1)]: AVATAR_URL });
	});

	it('says nothing where the track has no cover either', async () => {
		render(playlistPage({ rows: [playlistRow({ artist: 'A1', title: 'T1', href: permalink(1), artwork: '' })] }), SET);
		hydrationScript([apiClientEntry(), playlistEntry([track(1)])]);
		mockFetch();
		await loadContentScript();

		expect(artworkOf(await getDetails())).toEqual({ [permalink(1)]: '' });
	});

	it('keeps the thumbnail the page rendered, which the browser already has', async () => {
		const onPage = ART('shown');
		render(
			playlistPage({ rows: [playlistRow({ artist: 'A1', title: 'T1', href: permalink(1), artwork: onPage })] }),
			SET
		);
		hydrationScript([apiClientEntry(), playlistEntry([track(1, { artworkUrl: ART_URL })])]);
		mockFetch();
		await loadContentScript();

		expect(artworkOf(await getDetails())).toEqual({ [permalink(1)]: onPage });
	});

	it('leaves a row with no answer at all alone', async () => {
		render(playlistPage({ rows: [playlistRow({ artist: 'A1', title: 'T1', href: permalink(1), artwork: '' })] }), SET);
		hydrationScript([apiClientEntry()]);
		mockFetch(notThePlaylist(() => jsonResponse({ kind: 'user' })));
		await loadContentScript();

		const response = await getDetails();
		expect(response.tracks[0].details).toBeNull();
		expect(response.tracks[0].artworkUrl).toBe('');
	});
});
