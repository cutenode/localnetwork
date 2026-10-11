// src/tools/artists/artists.svelte.ts — the artists tool's list, kept for good.
//
// The panel around it is tested in test/panels/artists.test.ts; what is here
// is the store's own rules: what an entry keeps, what happens when the same
// artist comes in twice, which of two answers about an artist wins, how a
// lookup's answers are folded in and written back, and what it does when
// storage will not take it.
import { describe, it, expect, vi } from 'vitest';
import type { ProfileFailure, SavedArtistEntry } from '$lib/types';
import { flush } from '../helpers/flush.ts';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

// A fresh module graph each time, so the singleton starts empty.
async function freshArtists(storage = {}) {
	vi.resetModules();
	globalThis.chrome = createChrome({ storage });
	const module = await import('../../src/tools/artists/artists.svelte.ts');
	await module.artists.start();

	return module;
}

const stored = () => chrome.testStore('local').artists;

const NOW = Date.parse('2026-09-08T12:00:00Z');

const profile = (extra = {}) => ({
	id: 7,
	permalink: '/bicep',
	username: 'Bicep',
	fullName: '',
	city: 'Belfast',
	country: 'GB',
	followers: 10,
	trackCount: 1,
	verified: false,
	avatarUrl: '',
	bio: '',
	links: [],
	...extra
});

/** An artist as artistsOf() hands one over. */
const artist = (permalink: string, extra: Partial<SavedArtistEntry> = {}) => ({
	permalink,
	name: 'A',
	tracks: [{ permalink: `${permalink}/one`, title: 'One' }],
	...extra
});

describe('the list', () => {
	it('starts empty when storage holds nothing', async () => {
		const { artists } = await freshArtists();
		expect(artists.items).toEqual([]);
		expect(artists.count).toBe(0);
		expect(artists.sources).toEqual([]);
		expect(artists.has('/a')).toBe(false);
	});

	it('is read once, however many times it is started', async () => {
		const { artists } = await freshArtists();
		expect(chrome.storage.local.get).toHaveBeenCalledTimes(1);
		await artists.start();
		expect(chrome.storage.local.get).toHaveBeenCalledTimes(1);
	});

	it('keeps of an artist everything the panel draws a row from', async () => {
		const { artists } = await freshArtists();
		vi.setSystemTime(new Date(NOW));
		await artists.add([artist('/a', { profile: { ...profile(), at: NOW } })], { from: 'My Playlist' });
		expect(stored()).toEqual([
			{
				permalink: '/a',
				name: 'A',
				tracks: [{ permalink: '/a/one', title: 'One' }],
				profile: { ...profile(), at: NOW },
				from: 'My Playlist',
				at: NOW
			}
		]);
		expect(artists.sources).toEqual(['My Playlist']);
		expect(artists.entries).toEqual(stored());
	});

	it('fills in what a source left out, without claiming anything about it', async () => {
		const { artists } = await freshArtists();
		await artists.add([{ permalink: '/a' }]);
		expect(stored()[0]).toMatchObject({ name: '', tracks: [], profile: null, from: '' });
	});

	it('keeps an entry’s own source and time, for one out of a file', async () => {
		const { artists } = await freshArtists();
		await artists.add([{ permalink: '/a', from: 'elsewhere', at: 5 }], { from: 'here' });
		expect(stored()[0]).toMatchObject({ from: 'elsewhere', at: 5 });
	});

	it('takes only what a track is, of an artist’s tracks, and at most fifty of them', async () => {
		const { artists, TRACKS_MAX } = await freshArtists();
		const tracks = Array.from({ length: 60 }, (_, n) => ({ permalink: `/a/t${n}`, title: n }));
		await artists.add([
			// @ts-expect-error -- a null, a track with no path, and titles that are numbers
			{ permalink: '/a', tracks: [null, { title: 'no path' }, { permalink: '' }, ...tracks] }
		]);
		expect(stored()[0].tracks).toHaveLength(TRACKS_MAX);
		expect(stored()[0].tracks[0]).toEqual({ permalink: '/a/t0', title: '' });
		// @ts-expect-error -- tracks and a profile that are neither
		await artists.add([{ permalink: '/b', tracks: 'nope', profile: 'nope' }]);
		expect(stored()[1]).toMatchObject({ tracks: [], profile: null });
	});

	it('skips what is not an artist, and counts what it did', async () => {
		const { artists } = await freshArtists();
		expect(
			// @ts-expect-error -- a null and an entry with no path among the artists
			await artists.add([null, {}, { permalink: '/a/b' }, artist('/a'), artist('/a'), artist('/b')])
		).toEqual({ added: 2, refreshed: 0, refused: 0 });
		expect(artists.count).toBe(2);
	});

	it('writes nothing for an add that adds nothing', async () => {
		const { artists } = await freshArtists();
		expect(await artists.add([])).toEqual({ added: 0, refreshed: 0, refused: 0 });
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});

	it('refuses artists past the cap, and says how many', async () => {
		const { artists, MAX } = await freshArtists();
		const many = Array.from({ length: MAX + 2 }, (_, n) => artist(`/a${n}`));
		expect(await artists.add(many)).toEqual({ added: MAX, refreshed: 0, refused: 2 });
		// One already on the list still refreshes past the cap.
		expect(await artists.add([artist('/a0'), artist('/new')])).toEqual({
			added: 0,
			refreshed: 1,
			refused: 1
		});
	});
});

describe('the same artist twice', () => {
	it('refreshes rather than duplicates: the fresh name, both sets of tracks, the first source', async () => {
		const { artists } = await freshArtists();
		vi.setSystemTime(new Date(NOW));
		await artists.add([artist('/a', { name: 'A' })], { from: 'first' });
		vi.setSystemTime(new Date(NOW + 1000));
		expect(
			await artists.add(
				[
					artist('/a', {
						name: 'A2',
						tracks: [
							{ permalink: '/a/one', title: 'One' },
							{ permalink: '/a/two', title: 'Two' }
						]
					})
				],
				{ from: 'second' }
			)
		).toEqual({ added: 0, refreshed: 1, refused: 0 });
		expect(stored()).toEqual([
			{
				permalink: '/a',
				name: 'A2',
				tracks: [
					{ permalink: '/a/one', title: 'One' },
					{ permalink: '/a/two', title: 'Two' }
				],
				profile: null,
				from: 'first',
				at: NOW
			}
		]);
	});

	it('keeps the name it had when the fresh sighting has none', async () => {
		const { artists } = await freshArtists();
		await artists.add([artist('/a', { name: 'A' })]);
		await artists.add([artist('/a', { name: '' })]);
		expect(stored()[0].name).toBe('A');
	});

	it('caps the tracks of both sightings together', async () => {
		const { artists, TRACKS_MAX } = await freshArtists();
		const some = (from: number) => Array.from({ length: 40 }, (_, n) => ({ permalink: `/a/t${from + n}`, title: '' }));
		await artists.add([artist('/a', { tracks: some(0) })]);
		await artists.add([artist('/a', { tracks: some(30) })]);
		expect(stored()[0].tracks).toHaveLength(TRACKS_MAX);
		expect(stored()[0].tracks[0].permalink).toBe('/a/t0');
	});

	it('keeps the better of two answers about the account', async () => {
		const { betterProfile } = await freshArtists();
		const known = { ...profile(), at: 2 };
		const failed: ProfileFailure = { unavailable: true, at: 5 };
		expect(betterProfile(null, null)).toBe(null);
		expect(betterProfile(known, null)).toBe(known);
		expect(betterProfile(null, known)).toBe(known);
		expect(betterProfile(failed, known)).toBe(known);
		expect(betterProfile(known, failed)).toBe(known);
		const newer = { ...profile(), at: 3 };
		expect(betterProfile(known, newer)).toBe(newer);
		expect(betterProfile(newer, known)).toBe(newer);
		// Two answers with no time at all: the fresh one, as a refresh should.
		const timeless = { ...profile() };
		// @ts-expect-error -- two profiles with no `at`, which a stored one always has
		expect(betterProfile(timeless, { ...profile(), city: 'x' })).toEqual({ ...profile(), city: 'x' });
	});

	it('takes a profile from a second sighting, and keeps one over a sighting with none', async () => {
		const { artists } = await freshArtists();
		await artists.add([artist('/a')]);
		await artists.add([artist('/a', { profile: { ...profile(), at: 1 } })]);
		expect(stored()[0].profile).toEqual({ ...profile(), at: 1 });
		await artists.add([artist('/a')]);
		expect(stored()[0].profile).toEqual({ ...profile(), at: 1 });
	});
});

describe('what a lookup said', () => {
	it('is taken onto the entries it names, stamped with the time, and written a moment later', async () => {
		vi.useFakeTimers();
		vi.setSystemTime(new Date(NOW));
		const { artists, SAVE_DELAY_MS } = await freshArtists();
		await artists.add([artist('/a'), artist('/b'), artist('/c')]);
		chrome.storage.local.set.mockClear();

		expect(
			artists.learn([
				profile({ permalink: '/a' }),
				{ permalink: '/b', unavailable: true },
				{ permalink: '/nobody', unavailable: true },
				// @ts-expect-error -- a path that is not an artist's, answered with nothing
				{ permalink: '/a/b' },
				// @ts-expect-error -- no answer at all
				null
			])
		).toBe(2);
		expect(artists.items[0].profile).toEqual({ ...profile({ permalink: '/a' }), at: NOW });
		expect(artists.items[1].profile).toEqual({ unavailable: true, at: NOW });
		expect(artists.items[2].profile).toBe(null);
		// In memory at once; in storage after the delay — and once, however many
		// answers land in it.
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
		expect(artists.learn([profile({ permalink: '/c' })])).toBe(1);
		await vi.advanceTimersByTimeAsync(SAVE_DELAY_MS);
		expect(chrome.storage.local.set).toHaveBeenCalledTimes(1);
		expect(stored().map((one) => one.profile?.permalink ?? one.profile)).toEqual([
			'/a',
			{ unavailable: true, at: NOW },
			'/c'
		]);
	});

	it('is written now when flushed, and flushing with nothing pending writes nothing', async () => {
		vi.useFakeTimers();
		const { artists } = await freshArtists();
		await artists.add([artist('/a')]);
		chrome.storage.local.set.mockClear();
		await artists.flush();
		expect(chrome.storage.local.set).not.toHaveBeenCalled();

		artists.learn([profile({ permalink: '/a' })]);
		await artists.flush();
		expect(chrome.storage.local.set).toHaveBeenCalledTimes(1);
		expect(stored()[0].profile!.permalink).toBe('/a');
		// The timer went with the flush: nothing writes again later.
		await vi.advanceTimersByTimeAsync(5000);
		expect(chrome.storage.local.set).toHaveBeenCalledTimes(1);
	});

	it('takes nothing from an answer that names nobody on the list', async () => {
		const { artists } = await freshArtists();
		await artists.add([artist('/a')]);
		chrome.storage.local.set.mockClear();
		expect(artists.learn([profile({ permalink: '/zz' })])).toBe(0);
		// @ts-expect-error -- not a list of answers at all
		expect(artists.learn('nope')).toBe(0);
		await artists.flush();
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});
});

describe('taking artists off', () => {
	it('removes a whole set in one write', async () => {
		const { artists } = await freshArtists();
		await artists.add([artist('/a'), artist('/b'), artist('/c')]);
		chrome.storage.local.set.mockClear();
		await artists.removeMany(['/a', '/c', '/nobody']);
		expect(chrome.storage.local.set).toHaveBeenCalledTimes(1);
		expect(stored().map((one) => one.permalink)).toEqual(['/b']);
	});
});

describe('replacing the list', () => {
	it('makes it exactly the file’s, once per artist, cut off at the cap', async () => {
		const { artists, MAX } = await freshArtists({ artists: [artist('/gone')] });
		const many = Array.from({ length: MAX + 1 }, (_, n) => artist(`/a${n}`, { from: 'file', at: 1 }));
		// @ts-expect-error -- a null among the file's artists
		expect(await artists.replace([null, { permalink: 'bad' }, ...many, artist('/a0', { name: 'again' })])).toEqual({
			added: MAX,
			refused: 1
		});
		expect(artists.has('/gone')).toBe(false);
		expect(stored()[0]).toMatchObject({ permalink: '/a0', name: 'again', from: 'file', at: 1 });
	});

	it('empties the list for a file with none', async () => {
		const { artists } = await freshArtists({ artists: [artist('/gone')] });
		expect(await artists.replace([])).toEqual({ added: 0, refused: 0 });
		expect(stored()).toEqual([]);
	});
});

describe('what storage holds', () => {
	it('is read as the list, dropping whatever is not an artist', async () => {
		const { artists, listOf } = await freshArtists({
			artists: [artist('/a'), null, 'x', { permalink: '/a/b' }, { name: 'no path' }, artist('/b')]
		});

		expect(artists.items.map((one) => one.permalink)).toEqual(['/a', '/b']);
		expect(listOf('nope')).toEqual([]);
	});

	it('follows another context’s write, and ignores the rest', async () => {
		const { artists } = await freshArtists();
		chrome.testEmitStorageChange({ artists: { newValue: [artist('/a')] } }, 'local');
		expect(artists.items.map((one) => one.permalink)).toEqual(['/a']);
		chrome.testEmitStorageChange({ artists: { newValue: [artist('/b')] } }, 'session');
		chrome.testEmitStorageChange({ listed: { newValue: [] } }, 'local');
		expect(artists.items.map((one) => one.permalink)).toEqual(['/a']);
	});

	it('starts empty, in memory, when storage cannot be read', async () => {
		vi.resetModules();
		globalThis.chrome = createChrome();
		chrome.storage.local.get.mockRejectedValue(new Error('no storage'));
		const { artists } = await import('../../src/tools/artists/artists.svelte.ts');
		await artists.start();
		expect(artists.items).toEqual([]);
		expect(chrome.storage.onChanged.addListener).not.toHaveBeenCalled();
		await artists.add([artist('/a')]);
		expect(artists.count).toBe(1);
	});

	it('keeps the list on screen when a write fails', async () => {
		const { artists } = await freshArtists();
		chrome.storage.local.set.mockRejectedValue(new Error('QUOTA_BYTES quota exceeded'));
		await artists.add([artist('/a')]);
		expect(artists.count).toBe(1);
		expect(stored()).toBeUndefined();
		await flush();
	});
});
