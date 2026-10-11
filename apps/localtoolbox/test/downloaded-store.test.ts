// downloaded-store.ts — the set of downloaded tracks in extension storage.
import { describe, it, expect, beforeEach } from 'vitest';
import { createChrome, type ChromeMock } from './helpers/chrome.ts';
import { loadStore, type DownloadedStore } from './helpers/load.ts';

declare const chrome: ChromeMock;

const ISO = '2026-09-08T12:00:00.000Z';

const OLDER = '2026-01-01T00:00:00.000Z';

const NEWER = '2026-09-09T00:00:00.000Z';

describe('TbxDownloaded', () => {
	let store: DownloadedStore;
	beforeEach(async () => {
		store = await loadStore();
	});

	it('keeps the marks under the "downloaded" key', () => {
		expect(store.KEY).toBe('downloaded');
	});

	it('loads an empty map when nothing is stored, or when what is stored is not a map', async () => {
		expect(await store.load()).toEqual({});
		globalThis.chrome = createChrome({ storage: { downloaded: 'junk' } });
		expect(await store.load()).toEqual({});
		globalThis.chrome = createChrome({ storage: { downloaded: null } });
		expect(await store.load()).toEqual({});
		globalThis.chrome = createChrome({ storage: { downloaded: 7 } });
		expect(await store.load()).toEqual({});
		// An array is an object too, but one that would hold a mark as a named
		// property — which storage drops, so the mark would be gone on the way in.
		globalThis.chrome = createChrome({ storage: { downloaded: ['/x/y'] } });
		expect(await store.load()).toEqual({});
		await store.set('/a/b', true);
		expect(chrome.testStore().downloaded).toEqual({ '/a/b': ISO });
	});

	it('loads what is stored', async () => {
		globalThis.chrome = createChrome({ storage: { downloaded: { '/a/b': ISO } } });
		expect(await store.load()).toEqual({ '/a/b': ISO });
	});

	it('marks a track with the time it was marked, and unmarks it', async () => {
		await store.set('/a/b', true);
		expect(chrome.testStore().downloaded).toEqual({ '/a/b': ISO });
		await store.set('/c/d', true);
		await store.set('/a/b', false);
		expect(chrome.testStore().downloaded).toEqual({ '/c/d': ISO });
		await store.set('/never/marked', false);
		expect(chrome.testStore().downloaded).toEqual({ '/c/d': ISO });
	});

	it('marks several tracks in one write, ignoring empty permalinks', async () => {
		// @ts-expect-error -- a null among the permalinks, which is skipped like an empty one
		await store.setMany(['/a/b', '', null, '/c/d'], true);
		expect(chrome.storage.local.set).toHaveBeenCalledTimes(1);
		expect(chrome.testStore().downloaded).toEqual({ '/a/b': ISO, '/c/d': ISO });
		await store.setMany(['/a/b'], false);
		expect(chrome.testStore().downloaded).toEqual({ '/c/d': ISO });
	});

	it('writes nothing when there is nothing to mark', async () => {
		await store.setMany([], true);
		// @ts-expect-error -- an undefined permalink, which is skipped like an empty one
		await store.setMany([undefined, ''], true);
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});

	it('merges imported marks, keeping the time each carries', async () => {
		globalThis.chrome = createChrome({ storage: { downloaded: { '/a/b': ISO } } });
		const added = await store.merge({ '/c/d': OLDER, '/e/f': NEWER });
		expect(added).toBe(2);
		expect(chrome.testStore().downloaded).toEqual({ '/a/b': ISO, '/c/d': OLDER, '/e/f': NEWER });
	});

	it('keeps the earlier time for a track already marked, and does not count it as added', async () => {
		globalThis.chrome = createChrome({ storage: { downloaded: { '/a/b': ISO, '/c/d': ISO } } });
		const added = await store.merge({ '/a/b': OLDER, '/c/d': NEWER });
		expect(added).toBe(0);
		expect(chrome.testStore().downloaded).toEqual({ '/a/b': OLDER, '/c/d': ISO });
	});

	// Earlier as instants, not as text. A file may carry a time with an offset,
	// which sorts after a "Z" time an hour later than it — and one instant can
	// be written both ways, which is not a newer mark.
	it('keeps the earlier instant, however each time is written', async () => {
		const HOUR_BEFORE = '2026-09-08T13:00:00+02:00';
		const SAME_INSTANT = '2026-09-08T14:00:00+02:00';
		globalThis.chrome = createChrome({ storage: { downloaded: { '/a/b': ISO, '/c/d': ISO } } });
		expect(await store.merge({ '/a/b': HOUR_BEFORE, '/c/d': SAME_INSTANT })).toBe(0);
		expect(chrome.testStore().downloaded).toEqual({ '/a/b': HOUR_BEFORE, '/c/d': ISO });
	});

	it('writes nothing when there is nothing to merge', async () => {
		expect(await store.merge({})).toBe(0);
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});

	// The opt-in half of an import: what the file says, and nothing else. The
	// merge above is what happens unless somebody turns that switch on.
	it('replaces every mark with the ones it is given', async () => {
		globalThis.chrome = createChrome({ storage: { downloaded: { '/a/b': ISO, '/c/d': ISO } } });
		expect(await store.replaceAll({ '/c/d': OLDER, '/e/f': NEWER })).toBe(2);
		expect(chrome.testStore().downloaded).toEqual({ '/c/d': OLDER, '/e/f': NEWER });
	});

	it('replaces them with nothing at all when that is what the file holds', async () => {
		globalThis.chrome = createChrome({ storage: { downloaded: { '/a/b': ISO } } });
		expect(await store.replaceAll({})).toBe(0);
		expect(chrome.testStore().downloaded).toEqual({});
	});

	it('clears every mark', async () => {
		globalThis.chrome = createChrome({ storage: { downloaded: { '/a/b': ISO } } });
		await store.clear();
		expect(chrome.testStore().downloaded).toEqual({});
	});
});
