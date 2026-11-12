// content.ts, download details (the GET_DOWNLOAD_DETAILS message): what the
// page itself has loaded about each track. That is the hydration the page was
// served with, and every answer to the page's own API requests that
// page-data.ts hands over afterwards (here, `pageLoaded` plays that part). The
// content script asks SoundCloud for nothing, and keeps what it learns in the
// tab's memory only.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import { loadContentScript, disposeContentScripts } from '../helpers/load.ts';
import { mockFetch } from '../helpers/api.ts';
import { flush } from '../helpers/flush.ts';
import type { ChromeMock, Json } from '../helpers/chrome.ts';
import {
	ART,
	render,
	playlistPage,
	playlistRow,
	publicLikesPage,
	likedSound,
	hydrationScript,
	apiTrack,
	apiUser,
	pageLoaded,
	type ApiTrackOptions
} from '../helpers/dom.ts';
import type { ArtistProfile, DetailsAnswer } from '$lib/types';

declare const chrome: ChromeMock;

const NOW = new Date('2026-09-08T12:00:00Z').getTime();

beforeEach(() => {
	vi.useFakeTimers();
});

afterEach(disposeContentScripts);

const SET = '/curator/sets/my-playlist';

const row = (n: number) =>
	playlistRow({ artist: `A${n}`, title: `T${n}`, href: `/a${n}/t${n}?in=curator/sets/my-playlist` });

const permalink = (n: number) => `/a${n}/t${n}`;

const track = (n: number, extra: ApiTrackOptions = {}) => apiTrack({ id: n, permalink: permalink(n), ...extra });

/** A playlist page showing rows 1 to `count`. */
const playlistOf = (count: number) =>
	render(playlistPage({ rows: Array.from({ length: count }, (_, i) => row(i + 1)) }), SET);

const playlistEntry = (tracks: Json[]) => ({
	hydratable: 'playlist',
	data: { kind: 'playlist', permalink_url: `https://soundcloud.com${SET}`, tracks }
});

const askDetails = () => chrome.testSendMessage({ type: 'GET_DOWNLOAD_DETAILS' });

const detailsOf = (response: DetailsAnswer) => Object.fromEntries(response.tracks.map((t) => [t.permalink, t.details]));

/** Which of the page's rows have details: their ids, null for a row with none. */
const idsKnown = async () => (await askDetails()).tracks.map((t) => t.details?.id ?? null);

describe('what the page was served with', () => {
	it('keeps a track entry’s whole track, as the panel reads it', async () => {
		playlistOf(1);
		hydrationScript([
			{
				hydratable: 'sound',
				data: track(1, {
					downloadable: true,
					hasDownloadsLeft: false,
					purchaseUrl: 'https://hypeddit.com/one',
					purchaseTitle: '  Free   Download ',
					genre: ' Deep  House ',
					artworkUrl: ART('one', 'large')
				})
			}
		]);
		await loadContentScript();

		expect((await askDetails()).tracks[0].details).toEqual({
			id: 1,
			permalink: permalink(1),
			downloadable: true,
			hasDownloadsLeft: false,
			purchaseUrl: 'https://hypeddit.com/one',
			purchaseTitle: 'Free Download',
			genre: 'Deep House',
			artworkUrl: ART('one', 'large'),
			at: NOW
		});
	});

	it('keeps a playlist’s whole tracks and passes over its stubs and anything that is not a track', async () => {
		playlistOf(4);
		hydrationScript([
			{ hydratable: 'anonymousId', data: 'abc' },
			null,
			{ hydratable: 'broken', data: null },
			// No permalink: nothing to file it under.
			{ hydratable: 'sound', data: { kind: 'track', id: 9 } },
			{ hydratable: 'user', data: apiUser({ tracks: 'n/a' }) },
			// Past its first few tracks a playlist names the rest by id alone.
			playlistEntry([track(1), track(2, { downloadable: true }), { id: 3 }, {}, 'x', null])
		]);
		await loadContentScript();

		expect(await idsKnown()).toEqual([1, 2, null, null]);
	});

	it.each([
		['no hydration at all', null],
		['a script that never assigns it', 'var x = 1;'],
		['hydration that is not JSON', 'window.__sc_hydration = [{oops};'],
		['hydration that is not a list', 'window.__sc_hydration = {"hydratable":"sound"};']
	])('knows nothing yet from %s', async (_name, raw) => {
		playlistOf(1);

		if (raw !== null) hydrationScript(null, { raw });
		await loadContentScript();

		const response = await askDetails();
		expect(response.tracks).toHaveLength(1);
		expect(response.tracks[0].details).toBeNull();
	});

	it('reads the first inline hydration, ignoring external scripts and trailing markup', async () => {
		playlistOf(2);
		const external = document.createElement('script');
		external.src = 'https://a-v2.sndcdn.com/assets/app.js';
		external.textContent = `window.__sc_hydration = ${JSON.stringify([{ hydratable: 'sound', data: track(2) }])};`;
		document.head.append(external);
		hydrationScript(null, {
			raw: `\n  window.__sc_hydration = ${JSON.stringify([{ hydratable: 'sound', data: track(1) }])}\n</script><script>other();`
		});
		hydrationScript([{ hydratable: 'sound', data: track(2) }]);
		await loadContentScript();

		expect(await idsKnown()).toEqual([1, null]);
	});
});

describe('what the page loads afterwards', () => {
	it('learns a list of tracks by id', async () => {
		playlistOf(3);
		await loadContentScript();
		expect(await idsKnown()).toEqual([null, null, null]);

		pageLoaded('/tracks', [track(2), track(3), null, { kind: 'playlist', id: 5 }]);
		expect(await idsKnown()).toEqual([null, 2, 3]);
	});

	it('learns the tracks in a likes listing, liked playlists’ tracks included', async () => {
		playlistOf(3);
		await loadContentScript();

		pageLoaded('/users/42/track_likes', {
			collection: [
				{ track: track(1), created_at: '2026-01-01T00:00:00Z' },
				{ playlist: { kind: 'playlist', id: 7, tracks: [track(3), { id: 4 }] } },
				null
			],
			next_href: null
		});
		expect(await idsKnown()).toEqual([1, null, 3]);
	});

	it('learns a resolved track, and a playlist by id', async () => {
		playlistOf(3);
		await loadContentScript();

		pageLoaded('/resolve', track(1));
		pageLoaded('/playlists/5', { kind: 'playlist', id: 5, tracks: [track(2), { id: 3 }] });
		expect(await idsKnown()).toEqual([1, 2, null]);
	});

	it('takes the latest answer about a track', async () => {
		playlistOf(1);
		hydrationScript([{ hydratable: 'sound', data: track(1, { downloadable: true }) }]);
		await loadContentScript();

		// Spending the last download changes the answer the page gets.
		pageLoaded('/tracks', [track(1, { downloadable: true, hasDownloadsLeft: false })]);
		expect((await askDetails()).tracks[0].details).toMatchObject({ downloadable: true, hasDownloadsLeft: false });
	});

	it('lets by what is not an answer, and keeps listening after it', async () => {
		playlistOf(1);
		await loadContentScript();

		document.dispatchEvent(new Event('tbx:page-data'));

		for (const detail of [undefined, 42, { tracks: [track(1)] }, '/tracks\n[{oops', '/tracks\n']) {
			document.dispatchEvent(new CustomEvent('tbx:page-data', { detail }));
		}

		expect(await idsKnown()).toEqual([null]);

		pageLoaded('/tracks', [track(1)]);
		expect(await idsKnown()).toEqual([1]);
	});
});

/** Keeps the extension's storage from answering until the returned function is called. */
const holdStorage = () => {
	let release!: () => void;
	const gate = new Promise<void>((resolve) => (release = resolve));
	const realGet = chrome.storage.local.get.getMockImplementation()!;
	chrome.storage.local.get.mockImplementation(async (keys) => {
		await gate;

		return realGet(keys);
	});

	return release;
};

describe('telling page-data.ts it is listening', () => {
	it('says hello once, already listening, so what was held lands', async () => {
		playlistOf(1);
		let hellos = 0;

		// page-data.ts answers a hello by handing over what it held, at once.
		const replay = () => {
			hellos += 1;
			pageLoaded('/tracks', [track(1)]);
		};

		document.addEventListener('tbx:page-data-hello', replay);

		try {
			await loadContentScript();
		} finally {
			document.removeEventListener('tbx:page-data-hello', replay);
		}

		expect(hellos).toBe(1);
		expect(await idsKnown()).toEqual([1]);
	});

	// Learning repaints the markers, and a repaint before the settings are read
	// would put back buttons somebody turned off.
	it('waits for the extension’s storage before it says so', async () => {
		playlistOf(1);
		const release = holdStorage();
		let hellos = 0;
		const count = () => (hellos += 1);
		document.addEventListener('tbx:page-data-hello', count);

		try {
			await loadContentScript();
			expect(hellos).toBe(0);
			release();
			await flush();
			expect(hellos).toBe(1);
		} finally {
			document.removeEventListener('tbx:page-data-hello', count);
		}
	});
});

describe('a panel that asks while the script is starting', () => {
	// Until startup has read storage, the hydration has not been learned, so an
	// answer then would say nothing is loaded about a page that was served with
	// everything. The questions wait for startup instead.
	it('is answered with what the page was served with, once startup is done', async () => {
		playlistOf(1);
		hydrationScript([
			{ hydratable: 'user', data: apiUser({ permalink: 'a1', bio: '' }) },
			{ hydratable: 'sound', data: track(1) }
		]);
		const release = holdStorage();
		await loadContentScript();

		let details: DetailsAnswer | undefined;
		let profiles: ArtistProfile[] | undefined;
		void askDetails().then((answer) => (details = answer));
		void chrome
			.testSendMessage({ type: 'GET_ARTIST_PROFILES', permalinks: ['/a1'] })
			.then((answer) => (profiles = answer.profiles));
		await flush();
		expect(details).toBeUndefined();
		expect(profiles).toBeUndefined();

		release();
		await flush();
		expect(details!.tracks[0].details).toMatchObject({ id: 1, permalink: permalink(1) });
		expect(profiles!.map((profile) => profile.permalink)).toEqual(['/a1']);
	});

	// Learning the page needs none of the extension's own storage, so storage
	// that will not be read (an extension reloaded under the page) leaves the
	// panels hearing what the page loaded all the same.
	it('still learns the page when the extension’s storage cannot be read', async () => {
		playlistOf(1);
		hydrationScript([{ hydratable: 'sound', data: track(1) }]);
		chrome.storage.local.get.mockRejectedValue(new Error('Extension context invalidated.'));
		await loadContentScript();

		const answer = await askDetails();

		expect(answer.tracks[0].details).toMatchObject({ id: 1, permalink: permalink(1) });
	});
});

describe('the answer, row by row', () => {
	const likedRow = (n: number, purchase?: { url: string; label: string }, artwork?: string) =>
		likedSound({ title: `L${n}`, href: `/l${n}/t${n}`, purchase, artwork });

	const liked = (n: number) => `/l${n}/t${n}`;

	it('answers what the page loaded, else the purchase link the row shows, else nothing yet', async () => {
		render(
			publicLikesPage({
				user: 'dj',
				items: [
					likedRow(1, { url: 'https://hypeddit.com/one', label: 'Free Download' }),
					likedRow(2, { url: 'https://bandcamp.com/two', label: '' }),
					likedRow(3),
					likedRow(4, { url: 'https://buy.test/four', label: 'Buy' })
				]
			}),
			'/dj/likes'
		);
		hydrationScript([{ hydratable: 'sound', data: apiTrack({ id: 104, permalink: liked(4), downloadable: true }) }]);
		await loadContentScript();

		const response = await askDetails();
		expect(response.ok).toBe(true);
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
			null,
			// What the page loaded is the whole answer, and wins over the row's link.
			expect.objectContaining({ id: 104, downloadable: true, purchaseUrl: '' })
		]);
	});

	it('answers an empty page with no tracks', async () => {
		render('<p>nothing</p>', '/stream');
		await loadContentScript();
		expect(await askDetails()).toMatchObject({ ok: true, tracks: [] });
	});

	describe('cover art a row never loaded', () => {
		// A page lazy-loads its thumbnails as they scroll into view, so most rows
		// of a list read without scrolling through it carry no artwork URL at all.
		// The track object the page loaded names one.
		const ART_URL = 'https://i1.sndcdn.com/artworks-hash1-large.jpg';
		const AVATAR_URL = 'https://i1.sndcdn.com/avatars-hash2-large.jpg';

		const bareRow = (artwork = '') =>
			render(playlistPage({ rows: [playlistRow({ artist: 'A1', title: 'T1', href: permalink(1), artwork })] }), SET);

		const artworkOf = async () => (await askDetails()).tracks.map((t) => t.artworkUrl);

		it('takes the cover from the details', async () => {
			bareRow();
			hydrationScript([playlistEntry([track(1, { artworkUrl: ART_URL })])]);
			await loadContentScript();
			expect(await artworkOf()).toEqual([ART_URL]);
		});

		it('falls back to the uploader’s avatar, as the page’s own rows do', async () => {
			bareRow();
			hydrationScript([playlistEntry([track(1, { avatarUrl: AVATAR_URL })])]);
			await loadContentScript();
			expect(await artworkOf()).toEqual([AVATAR_URL]);
		});

		it('says nothing where the track has no cover either', async () => {
			bareRow();
			hydrationScript([playlistEntry([track(1)])]);
			await loadContentScript();
			expect(await artworkOf()).toEqual(['']);
		});

		it('keeps the thumbnail the page rendered, which the browser already has', async () => {
			bareRow(ART('shown'));
			hydrationScript([playlistEntry([track(1, { artworkUrl: ART_URL })])]);
			await loadContentScript();
			expect(await artworkOf()).toEqual([ART('shown')]);
		});

		it('leaves a row the page has loaded nothing about alone', async () => {
			bareRow();
			await loadContentScript();
			expect(await artworkOf()).toEqual(['']);
		});

		it('has no cover to give from the purchase link a row shows', async () => {
			render(
				publicLikesPage({ user: 'dj', items: [likedRow(1, { url: 'https://buy.test/one', label: 'Buy' }, '')] }),
				'/dj/likes'
			);
			await loadContentScript();
			const [one] = (await askDetails()).tracks;
			expect(one.details).toMatchObject({ partial: true });
			expect(one.artworkUrl).toBe('');
		});
	});
});

describe('the nodl pill', () => {
	const pills = () => [...document.querySelectorAll('.tbx-mark')].map((mark) => mark.textContent);

	it('appears when the page loads a track with no download link after the page was painted', async () => {
		playlistOf(2);
		await loadContentScript();
		expect(pills()).toEqual([]);

		pageLoaded('/tracks', [track(1), track(2, { downloadable: true })]);
		// The repaint is coalesced, as for the page's own rendering.
		await vi.advanceTimersByTimeAsync(150);
		expect(pills()).toEqual(['nodl']);
		expect(document.querySelector(`a[href^="${permalink(1)}"]`)!.closest('.trackItem')!.textContent).toContain('nodl');
	});
});

describe('what it never does', () => {
	it('asks SoundCloud for nothing, and keeps nothing it learned in the extension’s storage', async () => {
		const { fn } = mockFetch();
		playlistOf(3);
		hydrationScript([playlistEntry([track(1), { id: 2 }, { id: 3 }])]);
		await loadContentScript();

		pageLoaded('/tracks', [track(2)]);
		await askDetails();
		await askDetails();
		await vi.advanceTimersByTimeAsync(60_000);

		expect(fn).not.toHaveBeenCalled();
		expect(chrome.testStore()).not.toHaveProperty('details');
		const written = chrome.storage.local.set.mock.calls.flatMap(([items]) => Object.keys(items));
		expect(written).not.toContain('details');
		expect(detailsOf(await askDetails())[permalink(3)]).toBeNull();
	});
});
