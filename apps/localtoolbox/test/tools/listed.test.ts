// src/tools/manager/listed.svelte.ts — the Manager's list, kept for good.
//
// The panel around it is tested in test/panels/manager.test.ts; what is here
// is the store's own rules: what an entry keeps, what happens when the same
// track comes in twice, which of two answers about a track wins, and what it
// does when storage will not take it.
import { describe, it, expect, vi } from 'vitest';
import type { ListedEntry, PartialDetails, Track } from '$lib/types';
import { flush } from '../helpers/flush.ts';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

// A fresh module graph each time, so the singleton starts empty.
async function freshListed(storage = {}) {
	vi.resetModules();
	globalThis.chrome = createChrome({ storage });
	const module = await import('../../src/tools/manager/listed.svelte.ts');
	await module.listed.start();

	return module;
}

const stored = () => chrome.testStore('local').listed;

const details = (extra = {}) => ({
	id: 1,
	permalink: '/a/one',
	downloadable: true,
	hasDownloadsLeft: true,
	purchaseUrl: '',
	purchaseTitle: '',
	at: 1,
	...extra
});

/** What a Likes row renders by itself: half an answer, and it says so. */
const partial = (extra = {}): PartialDetails => ({
	id: null,
	permalink: '/a/one',
	downloadable: null,
	hasDownloadsLeft: null,
	purchaseUrl: 'https://hypeddit.com/one',
	purchaseTitle: '',
	partial: true,
	...extra
});

/** A track as a page hands one over. */
const track = (permalink: string, extra: Partial<Track & Pick<ListedEntry, 'from' | 'at'>> = {}) => ({
	permalink,
	title: 'One',
	artist: 'A',
	artworkUrl: 'https://i1.sndcdn.com/artworks-one-t120x120.jpg',
	details: null,
	...extra
});

describe('the list', () => {
	it('starts empty when storage holds nothing', async () => {
		const { listed } = await freshListed();
		expect(listed.items).toEqual([]);
		expect(listed.count).toBe(0);
		expect(listed.sources).toEqual([]);
		expect(listed.has('/a/one')).toBe(false);
	});

	it('is read once, however many times it is started', async () => {
		const { listed } = await freshListed();
		expect(chrome.storage.local.get).toHaveBeenCalledTimes(1);
		await listed.start();
		expect(chrome.storage.local.get).toHaveBeenCalledTimes(1);
	});

	it('keeps of a track everything the panel draws a row from', async () => {
		const { listed } = await freshListed();
		vi.setSystemTime(new Date('2026-09-08T12:00:00Z'));
		await listed.add(
			[
				track('/a/one', {
					details: details(),
					genre: 'Techno',
					liked: true,
					reposted: false,
					domPurchaseTitle: 'Free Download'
				})
			],
			{ from: 'My Playlist' }
		);

		expect(stored()).toEqual([
			{
				permalink: '/a/one',
				title: 'One',
				artist: 'A',
				artworkUrl: 'https://i1.sndcdn.com/artworks-one-t120x120.jpg',
				details: details(),
				genre: 'Techno',
				liked: true,
				reposted: false,
				domPurchaseTitle: 'Free Download',
				from: 'My Playlist',
				at: Date.parse('2026-09-08T12:00:00Z')
			}
		]);
		expect(listed.sources).toEqual(['My Playlist']);
	});

	// A page can render a row with none of this — a tile with no artist, a
	// track page that never said whether you liked it. Absent is not false.
	it('fills in what a page left out, without claiming anything about it', async () => {
		const { listed } = await freshListed();
		await listed.add([{ permalink: '/a/one' }]);
		const [entry] = stored();
		expect(entry).toMatchObject({
			title: '',
			artist: '',
			artworkUrl: '',
			details: null,
			genre: null,
			liked: null,
			reposted: null,
			domPurchaseTitle: '',
			from: ''
		});
		// Nothing to file it under, so the list names no source for it.
		expect(listed.sources).toEqual([]);
	});

	it('refuses a track with no permalink, which is nothing it could tell apart', async () => {
		const { listed } = await freshListed();
		// @ts-expect-error -- a null among the tracks
		const result = await listed.add([{ permalink: '' }, null, track('/a/one')]);
		expect(result).toEqual({ added: 1, refreshed: 0, refused: 0 });
		expect(listed.items.map((one) => one.permalink)).toEqual(['/a/one']);
	});

	it('writes once however many tracks are added, and not at all when none are', async () => {
		const { listed } = await freshListed();
		await listed.add([track('/a/one'), track('/b/two'), track('/c/three')]);
		expect(chrome.storage.local.set).toHaveBeenCalledTimes(1);
		expect(await listed.add([])).toEqual({ added: 0, refreshed: 0, refused: 0 });
		expect(chrome.storage.local.set).toHaveBeenCalledTimes(1);
	});

	// A playlist with the same track twice, and adding the same page again:
	// one permalink is one entry, in the place it first took.
	it('holds a track once, wherever it came in twice', async () => {
		const { listed } = await freshListed();
		vi.setSystemTime(new Date('2026-09-08T12:00:00Z'));
		await listed.add([track('/a/one'), track('/b/two'), track('/a/one', { title: 'again' })], {
			from: 'First'
		});
		expect(listed.items.map((one) => one.permalink)).toEqual(['/a/one', '/b/two']);

		vi.setSystemTime(new Date('2026-09-09T12:00:00Z'));

		const result = await listed.add([track('/a/one', { title: 'renamed' }), track('/c/three')], {
			from: 'Second'
		});

		expect(result).toEqual({ added: 1, refreshed: 1, refused: 0 });
		expect(listed.items.map((one) => one.permalink)).toEqual(['/a/one', '/b/two', '/c/three']);

		// The words and the picture are the fresh ones — a page may have been
		// renamed since — but when it came on, and off what, are not rewritten.
		const [first] = listed.items;
		expect(first.title).toBe('renamed');
		expect(first.from).toBe('First');
		expect(first.at).toBe(Date.parse('2026-09-08T12:00:00Z'));
		expect(listed.sources).toEqual(['First', 'Second']);
	});

	it('takes a whole set of tracks off in one write', async () => {
		const { listed } = await freshListed();
		await listed.add([track('/a/one'), track('/b/two'), track('/c/three')]);
		chrome.storage.local.set.mockClear();
		await listed.removeMany(['/a/one', '/c/three', '/never/listed']);
		expect(chrome.storage.local.set).toHaveBeenCalledTimes(1);
		expect(listed.items.map((one) => one.permalink)).toEqual(['/b/two']);
	});

	// The other half of an import, which nobody reaches without turning a switch
	// on: the list becomes the file's list and what was on it is gone.
	it('becomes exactly the file’s list when the import is asked to replace', async () => {
		const { listed } = await freshListed();
		await listed.add([track('/a/one'), track('/b/two')], { from: 'A page' });

		expect(await listed.replace([track('/b/two', { title: 'from the file' }), track('/c/three')])).toEqual({
			added: 2,
			refused: 0
		});
		expect(listed.items.map((one) => one.permalink)).toEqual(['/b/two', '/c/three']);
		// The file's words, not the ones that were on the list: replacing means
		// the file is what is left.
		expect(listed.items[0].title).toBe('from the file');
		expect(listed.items[0].from).toBe('');
		expect(stored().map((one) => one.permalink)).toEqual(['/b/two', '/c/three']);
	});

	it('folds a track the file lists twice into one, and empties for an empty file', async () => {
		const { listed } = await freshListed();
		await listed.add([track('/a/one')]);

		expect(
			await listed.replace([
				track('/b/two'),
				track('/b/two', { artworkUrl: '', details: details({ permalink: '/b/two' }) })
			])
		).toEqual({ added: 1, refused: 0 });
		expect(listed.count).toBe(1);
		// The second sighting refreshed the first rather than adding to it: the
		// cover it did not show is still there, and the answer it brought is.
		expect(listed.items[0].artworkUrl).toBe('https://i1.sndcdn.com/artworks-one-t120x120.jpg');
		expect(listed.items[0].details).toEqual(details({ permalink: '/b/two' }));

		expect(await listed.replace([])).toEqual({ added: 0, refused: 0 });
		expect(listed.items).toEqual([]);
	});

	it('refuses what will not fit when it is replacing too', async () => {
		const { listed, MAX } = await freshListed();
		const many = Array.from({ length: MAX + 2 }, (_, i) => track(`/a/t${i}`));
		expect(await listed.replace(many)).toEqual({ added: MAX, refused: 2 });
		expect(listed.count).toBe(MAX);
	});

	it('drops what cannot be an entry, whichever way it is being restored', async () => {
		const { listed } = await freshListed();
		// @ts-expect-error -- a null and an entry with no permalink among the file's
		expect(await listed.replace([null, { title: 'no permalink' }, track('/a/one')])).toEqual({
			added: 1,
			refused: 0
		});
		expect(listed.items.map((one) => one.permalink)).toEqual(['/a/one']);
	});

	// The two the list itself knows — where a track came in from, and when.
	// A page carries neither, so they are stamped; an entry out of an exported
	// file carries both, and a restore that overwrote them would lose exactly
	// what the file was written to keep.
	it('keeps a restored entry’s own source and time rather than stamping them', async () => {
		const { listed } = await freshListed();
		const fromFile = track('/a/one', { from: 'My Playlist', at: 1700000000000 });

		await listed.replace([fromFile]);
		expect(listed.items[0].from).toBe('My Playlist');
		expect(listed.items[0].at).toBe(1700000000000);

		const again = await freshListed();
		await again.listed.add([fromFile]);
		expect(again.listed.items[0].from).toBe('My Playlist');
		expect(again.listed.items[0].at).toBe(1700000000000);
		expect(again.listed.sources).toEqual(['My Playlist']);
	});

	// Five thousand is the ceiling, and past it the list refuses rather than
	// dropping what is already on it: these are tracks somebody put here.
	it('refuses what will not fit rather than dropping the oldest', async () => {
		const { listed, MAX } = await freshListed();
		const many = Array.from({ length: MAX }, (_, i) => track(`/a/t${i}`));
		expect(await listed.add(many)).toEqual({ added: MAX, refreshed: 0, refused: 0 });

		expect(await listed.add([track('/a/t0'), track('/b/over'), track('/c/over')])).toEqual({
			added: 0,
			refreshed: 1,
			refused: 2
		});
		expect(listed.count).toBe(MAX);
		expect(listed.has('/b/over')).toBe(false);
		// The first one on is still on: nothing was made room for.
		expect(listed.has('/a/t0')).toBe(true);
	});
});

describe('which of two answers about a track it keeps', () => {
	it('prefers any answer to none, and a whole one to half', async () => {
		const { betterDetails } = await freshListed();
		const known = details();
		const half = partial();

		expect(betterDetails(null, null)).toBe(null);
		expect(betterDetails(known, null)).toBe(known);
		expect(betterDetails(null, known)).toBe(known);
		// A Likes row's answer says nothing about SoundCloud's own download, so
		// it never displaces one that does.
		expect(betterDetails(half, known)).toBe(known);
		expect(betterDetails(known, half)).toBe(known);
		// Two of the same strength: the newer, which is what makes adding a page
		// again a refresh rather than a no-op.
		const newer = details({ at: 2 });
		expect(betterDetails(known, newer)).toBe(newer);
		expect(betterDetails(half, partial({ purchaseUrl: 'https://hypeddit.com/two' }))).not.toBe(half);
	});

	it('applies that rule when a page is added over a track already listed', async () => {
		const { listed } = await freshListed();
		await listed.add([track('/a/one', { details: partial() })]);
		await listed.add([track('/a/one', { details: details() })]);
		expect(listed.items[0].details).toEqual(details());

		// And the other way round, the half answer leaves the whole one alone.
		await listed.add([track('/a/one', { details: partial() })]);
		expect(listed.items[0].details).toEqual(details());

		// What the page said about a like, or a genre, is kept when a later
		// sighting is silent — the badge tiles render neither.
		await listed.add([track('/a/one', { liked: true, reposted: true, genre: 'Techno' })]);
		await listed.add([track('/a/one', { liked: null, reposted: false, genre: null })]);
		expect(listed.items[0]).toMatchObject({ liked: true, reposted: false, genre: 'Techno' });
		await listed.add([track('/a/one', { genre: 'House' })]);
		expect(listed.items[0].genre).toBe('House');
	});
});

describe('what it does with storage it cannot trust', () => {
	it('reads back only what is an entry', async () => {
		const { listed } = await freshListed({
			listed: [track('/a/one'), { title: 'no permalink' }, { permalink: '' }, null, 'not an entry']
		});

		expect(listed.items.map((one) => one.permalink)).toEqual(['/a/one']);
	});

	it('starts empty when what is stored is not a list at all', async () => {
		const { listed } = await freshListed({ listed: { permalink: '/a/one' } });
		expect(listed.items).toEqual([]);
	});

	it('starts empty when storage will not be read', async () => {
		vi.resetModules();
		globalThis.chrome = createChrome();
		chrome.storage.local.get.mockRejectedValue(new Error('no storage'));
		const { listed } = await import('../../src/tools/manager/listed.svelte.ts');
		await listed.start();
		expect(listed.items).toEqual([]);
		// And it follows nothing, since there is nothing to follow.
		expect(chrome.storage.onChanged.addListener).not.toHaveBeenCalled();
	});

	// The list on screen is the list in memory: a write that cannot happen
	// costs it only its life beyond this popup.
	it('shows what it holds even when the write fails', async () => {
		const { listed } = await freshListed();
		chrome.storage.local.set.mockRejectedValue(new Error('quota exceeded'));
		await listed.add([track('/a/one')]);
		expect(listed.items.map((one) => one.permalink)).toEqual(['/a/one']);
		expect(listed.entries).toEqual(listed.items.map((one) => ({ ...one })));
	});

	// Another window's popup, or an import, moving it under us.
	it('follows the list as storage changes, and ignores other keys', async () => {
		const { listed } = await freshListed();
		await listed.add([track('/a/one')]);

		chrome.testEmitStorageChange({ listed: { newValue: [track('/b/two'), { nope: true }] } });
		await flush();
		expect(listed.items.map((one) => one.permalink)).toEqual(['/b/two']);

		chrome.testEmitStorageChange({ downloaded: { newValue: { '/c/three': 'now' } } });
		await flush();
		expect(listed.items.map((one) => one.permalink)).toEqual(['/b/two']);

		// The same key in the session area is another key altogether.
		chrome.testEmitStorageChange({ listed: { newValue: [] } }, 'session');
		await flush();
		expect(listed.items.map((one) => one.permalink)).toEqual(['/b/two']);

		// A key emptied elsewhere leaves nothing behind either.
		chrome.testEmitStorageChange({ listed: { newValue: undefined } });
		await flush();
		expect(listed.items).toEqual([]);
	});
});
