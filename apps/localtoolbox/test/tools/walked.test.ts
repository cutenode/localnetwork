// src/tools/third-party-downloads/walked.svelte.ts — the Downloader's list,
// kept for the browsing session.
//
// The panel around it is tested in test/panels/third-party-downloads.test.ts
// and test/panels/remembered.test.ts; what is here is the store's own rules:
// what a row keeps, what it deliberately does not, and what happens to a row
// whose site this version no longer knows.
import { describe, it, expect, vi } from 'vitest';
import type { Site } from '../../src/tools/third-party-downloads/sites/index.ts';
import type { Row } from '../../src/tools/third-party-downloads/walked.svelte.ts';
import type { Track } from '$lib/types';
import { flush } from '../helpers/flush.ts';
import { createChrome, type ChromeMock, type Seed } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

const HYPE = 'https://hypeddit.com/886rp1';

const DROP = 'https://droploud.com/gate/abc';

// A fresh module graph each time, so the singleton starts empty.
async function freshWalked(session: Seed = {}) {
	vi.resetModules();
	globalThis.chrome = createChrome({ session });
	const { walked } = await import('../../src/tools/third-party-downloads/walked.svelte.ts');
	const { siteFor } = await import('../../src/tools/third-party-downloads/sites/index.ts');
	await walked.start();

	return { walked, siteFor };
}

const stored = () => chrome.testStore('session').downloader;

/** What a test sets on a row: any of its fields, and a track by its permalink alone. */
type RowFields = Omit<Partial<Row>, 'track'> & { track?: Pick<Track, 'permalink'> | null };

/** A row as the panel builds one. */
const row = (url: string, site: Site | null, extra: RowFields = {}) =>
	// SAFETY: every link here is one a site claims, so `site` is never null, and the store keeps a
	// row's `track` whole without reading into it, so one carrying only its permalink stands in.
	({
		url,
		site,
		permalink: '',
		track: null,
		label: 'a-label',
		byHand: false,
		state: 'queued',
		detail: '',
		openUrl: '',
		next: '',
		...extra
	}) as Row;

describe('the list', () => {
	it('starts empty when the session holds nothing', async () => {
		const { walked } = await freshWalked();
		expect(walked.rows).toEqual([]);
		expect(walked.origin).toBe('');
		expect(walked.byHand).toEqual([]);
		expect(walked.listed(HYPE)).toBe(null);
	});

	it('is read once, however many times it is started', async () => {
		const { walked } = await freshWalked();
		expect(chrome.storage.session.get).toHaveBeenCalledTimes(1);
		await walked.start();
		expect(chrome.storage.session.get).toHaveBeenCalledTimes(1);
	});

	it('keeps of a row what the panel would have to ask for again', async () => {
		const { walked, siteFor } = await freshWalked();
		walked.origin = 'My Playlist';
		walked.add(row(HYPE, siteFor(HYPE), { permalink: '/a/one', track: { permalink: '/a/one' } }));
		await flush();

		expect(stored()).toEqual({
			origin: 'My Playlist',
			listed: [HYPE],
			known: {
				[HYPE]: {
					url: HYPE,
					permalink: '/a/one',
					track: { permalink: '/a/one' },
					label: 'a-label',
					byHand: false,
					state: 'queued',
					detail: '',
					openUrl: '',
					next: ''
				}
			}
		});
		// The site is a module with functions on it, which no storage takes.
		expect(stored().known[HYPE]).not.toHaveProperty('site');
	});

	it('writes once per turn, however many rows moved in it', async () => {
		const { walked, siteFor } = await freshWalked();
		walked.add(row(HYPE, siteFor(HYPE)));
		walked.add(row(DROP, siteFor(DROP)));
		walked.origin = 'My Playlist';
		walked.save();
		await flush();
		expect(chrome.storage.session.set).toHaveBeenCalledTimes(1);
		expect(stored().listed).toEqual([HYPE, DROP]);
	});

	it('gives a row back with its site, and the row it hands out is the live one', async () => {
		const { walked, siteFor } = await freshWalked();
		const added = walked.add(row(HYPE, siteFor(HYPE)));
		added.state = 'done';
		added.label = 'A Track.wav';
		walked.save();
		await flush();

		expect(walked.listed(HYPE)!.state).toBe('done');
		expect(stored().known[HYPE].state).toBe('done');
		expect(stored().known[HYPE].label).toBe('A Track.wav');
	});

	it('comes back as the list it was, sites and all', async () => {
		const { walked } = await freshWalked({
			downloader: {
				origin: 'My Playlist',
				listed: [HYPE],
				known: { [HYPE]: { url: HYPE, state: 'done', label: 'A Track.wav', byHand: true } }
			}
		});

		expect(walked.origin).toBe('My Playlist');
		expect(walked.rows).toHaveLength(1);
		expect(walked.rows[0].site.name).toBe('Hypeddit');
		expect(walked.rows[0].state).toBe('done');
		expect(walked.byHand).toEqual([HYPE]);
	});

	it('reads a stored list that names neither an origin nor its rows', async () => {
		const { walked } = await freshWalked({ downloader: {} });
		expect(walked.origin).toBe('');
		expect(walked.rows).toEqual([]);
	});

	it('drops a row whose site this version no longer knows', async () => {
		const { walked } = await freshWalked({
			downloader: {
				listed: ['https://gone.example/gate/1', HYPE],
				known: {
					'https://gone.example/gate/1': { url: 'https://gone.example/gate/1', state: 'queued' },
					[HYPE]: { url: HYPE, state: 'queued' }
				}
			}
		});

		expect(walked.rows.map((r) => r.url)).toEqual([HYPE]);
	});

	it('drops a listed link it has no row for at all', async () => {
		const { walked } = await freshWalked({ downloader: { listed: [HYPE], known: {} } });
		expect(walked.rows).toEqual([]);
	});
});

describe('a link that has been walked before', () => {
	it('goes back on as the row it was, whichever source offers it', async () => {
		const { walked, siteFor } = await freshWalked();
		const added = walked.add(row(HYPE, siteFor(HYPE)));
		added.state = 'done';
		walked.clear();
		await flush();
		expect(walked.rows).toEqual([]);
		expect(walked.origin).toBe('');

		const again = walked.add(row(HYPE, siteFor(HYPE)));
		expect(again.state).toBe('done');
		expect(again.site.name).toBe('Hypeddit');
	});

	it('takes the track the page has for it when it was pasted in without one', async () => {
		const { walked, siteFor } = await freshWalked();
		walked.add(row(HYPE, siteFor(HYPE), { byHand: true, label: '886rp1' }));
		walked.clear();

		const again = walked.add(
			row(HYPE, siteFor(HYPE), {
				permalink: '/a/one',
				track: { permalink: '/a/one' },
				label: 'Artist — Track'
			})
		);

		expect(again.permalink).toBe('/a/one');
		expect(again.track).toEqual({ permalink: '/a/one' });
		expect(again.label).toBe('Artist — Track');
		// Pasted in by hand it was, and pasted in by hand it stays: no source will
		// offer it again if the page it turned out to be on is closed.
		expect(again.byHand).toBe(true);
	});

	it('keeps the name the site gave a row that has already been walked', async () => {
		const { walked, siteFor } = await freshWalked();
		const added = walked.add(row(HYPE, siteFor(HYPE), { byHand: true }));
		added.state = 'done';
		added.label = 'A Track.wav';
		walked.clear();

		const again = walked.add(row(HYPE, siteFor(HYPE), { permalink: '/a/one', label: 'Artist — Track' }));
		expect(again.label).toBe('A Track.wav');
		expect(again.permalink).toBe('/a/one');
	});

	it('keeps the track it already had', async () => {
		const { walked, siteFor } = await freshWalked();
		walked.add(row(HYPE, siteFor(HYPE), { permalink: '/a/one', track: { permalink: '/a/one' } }));
		walked.clear();

		const again = walked.add(row(HYPE, siteFor(HYPE), { permalink: '/b/two' }));
		expect(again.permalink).toBe('/a/one');
	});
});

describe('a walk that was still going', () => {
	// The run died with the popup that started it: nothing was saved, nothing
	// failed, and the row is the one it was before Get was pressed.
	it('is kept as the row it was before it began', async () => {
		const { walked, siteFor } = await freshWalked();
		const one = walked.add(row(HYPE, siteFor(HYPE)));
		const two = walked.add(row(DROP, siteFor(DROP)));
		one.state = 'working';
		two.state = 'saving';
		walked.save();
		await flush();

		expect(stored().known[HYPE].state).toBe('queued');
		expect(stored().known[DROP].state).toBe('queued');
	});
});

describe('with no session storage to write to', () => {
	it('holds the list for as long as the popup is open, and says nothing', async () => {
		vi.resetModules();
		globalThis.chrome = createChrome();
		chrome.storage.session.get.mockRejectedValue(new Error('no session storage'));
		chrome.storage.session.set.mockRejectedValue(new Error('no session storage'));
		const { walked } = await import('../../src/tools/third-party-downloads/walked.svelte.ts');
		const { siteFor } = await import('../../src/tools/third-party-downloads/sites/index.ts');

		await expect(walked.start()).resolves.toBeUndefined();
		walked.add(row(HYPE, siteFor(HYPE)));
		await flush();
		expect(walked.listed(HYPE)!.url).toBe(HYPE);
	});
});
