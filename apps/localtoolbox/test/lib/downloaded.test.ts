// src/lib/downloaded.svelte.ts — the reactive mirror of the marks store.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { flush } from '../helpers/flush.ts';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

const ISO = '2026-09-08T12:00:00.000Z';

// A fresh module graph each time, so the singleton starts empty.
async function freshStore(storage = {}) {
	vi.resetModules();
	globalThis.chrome = createChrome({ storage });
	// @ts-expect-error -- a classic script, run for the global it defines
	await import('../../downloaded-store.ts');
	const { downloaded } = await import('../../src/lib/downloaded.svelte.ts');
	await downloaded.start();

	return downloaded;
}

beforeEach(() => {
	vi.setSystemTime(new Date('2026-09-08T12:00:00Z'));
});

describe('the marks', () => {
	it('reads what is stored, and says what it holds', async () => {
		const downloaded = await freshStore({ downloaded: { '/a/one': ISO } });
		expect(downloaded.map).toEqual({ '/a/one': ISO });
		expect(downloaded.count).toBe(1);
		expect(downloaded.has('/a/one')).toBe(true);
		expect(downloaded.has('/b/two')).toBe(false);
	});

	it('is read once, however many panels ask', async () => {
		const downloaded = await freshStore();
		expect(chrome.storage.local.get).toHaveBeenCalledTimes(1);
		await downloaded.start();
		expect(chrome.storage.local.get).toHaveBeenCalledTimes(1);
		expect(chrome.storage.onChanged.addListener).toHaveBeenCalledTimes(1);
	});

	it('marks, unmarks and toggles', async () => {
		const downloaded = await freshStore();
		await downloaded.set('/a/one', true);
		await flush();
		expect(downloaded.has('/a/one')).toBe(true);

		await downloaded.toggle('/a/one');
		await flush();
		expect(downloaded.has('/a/one')).toBe(false);

		await downloaded.setMany(['/b/two', '/c/three'], true);
		await flush();
		expect(Object.keys(downloaded.map).sort()).toEqual(['/b/two', '/c/three']);

		await downloaded.clear();
		await flush();
		expect(downloaded.count).toBe(0);
	});

	it('follows a change made in another window, and ignores the rest', async () => {
		const downloaded = await freshStore();
		chrome.testEmitStorageChange({ downloaded: { newValue: { '/a/one': ISO } } });
		expect(downloaded.has('/a/one')).toBe(true);

		chrome.testEmitStorageChange({ downloaded: { newValue: { '/b/two': ISO } } }, 'sync');
		chrome.testEmitStorageChange({ details: { newValue: {} } });
		expect(downloaded.map).toEqual({ '/a/one': ISO });

		chrome.testEmitStorageChange({ downloaded: { newValue: undefined } });
		expect(downloaded.count).toBe(0);
	});

	it('starts empty, and stops following, when storage cannot be read', async () => {
		vi.resetModules();
		globalThis.chrome = createChrome();
		chrome.storage.local.get.mockRejectedValue(new Error('Extension context invalidated'));
		// @ts-expect-error -- a classic script, run for the global it defines
		await import('../../downloaded-store.ts');
		const { downloaded } = await import('../../src/lib/downloaded.svelte.ts');
		await downloaded.start();
		expect(downloaded.map).toEqual({});
		expect(chrome.storage.onChanged.addListener).not.toHaveBeenCalled();
	});
});
