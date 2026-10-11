// src/lib/details.svelte.ts — how many tracks have been looked up.
//
// The cache itself belongs to the content script; this is the one number the
// popup has any use for, and the settings sheet shows it beside the marks and
// the list so that "export" says what it would be saving.
import { describe, it, expect, vi } from 'vitest';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

const entry = (permalink: string) => ({
	id: 1,
	permalink,
	downloadable: true,
	hasDownloadsLeft: true,
	purchaseUrl: '',
	purchaseTitle: '',
	at: 1757332800000
});

// A fresh module graph each time, so the singleton starts empty.
async function freshDetails(storage = {}) {
	vi.resetModules();
	globalThis.chrome = createChrome({ storage });
	// @ts-expect-error -- a classic script, run for the global it defines
	await import('../../state-transfer.ts');
	const { details } = await import('../../src/lib/details.svelte.ts');
	await details.start();

	return details;
}

describe('the looked-up tracks', () => {
	it('counts what is cached', async () => {
		const details = await freshDetails({
			details: { '/a/one': entry('/a/one'), '/b/two': { unavailable: true, at: 1 } }
		});

		expect(details.count).toBe(2);
	});

	it('is read once, however many things ask', async () => {
		const details = await freshDetails();
		expect(details.count).toBe(0);
		expect(chrome.storage.local.get).toHaveBeenCalledTimes(1);
		await details.start();
		expect(chrome.storage.local.get).toHaveBeenCalledTimes(1);
		expect(chrome.storage.onChanged.addListener).toHaveBeenCalledTimes(1);
	});

	// Storage is not this version's to trust, and neither is an older install's
	// cache: anything that is not a map of entries counts as nothing rather than
	// throwing under a number nobody would miss.
	it.each([
		['nothing stored', undefined],
		['something that is not an object', 'junk'],
		['a list', ['/a/one', '/b/two']],
		['null', null]
	])('counts nothing for %s', async (_what, stored) => {
		const details = await freshDetails(stored === undefined ? {} : { details: stored });
		expect(details.count).toBe(0);
	});

	it('follows a lookup made in a SoundCloud tab, and ignores everything else', async () => {
		const details = await freshDetails();
		chrome.testEmitStorageChange({ details: { newValue: { '/a/one': entry('/a/one') } } });
		expect(details.count).toBe(1);

		chrome.testEmitStorageChange({ details: { newValue: {} } }, 'sync');
		chrome.testEmitStorageChange({ downloaded: { newValue: { '/b/two': 'x' } } });
		expect(details.count).toBe(1);

		chrome.testEmitStorageChange({ details: { newValue: undefined } });
		expect(details.count).toBe(0);
	});

	it('counts nothing, and stops following, when storage cannot be read', async () => {
		vi.resetModules();
		globalThis.chrome = createChrome();
		chrome.storage.local.get.mockRejectedValue(new Error('Extension context invalidated'));
		// @ts-expect-error -- a classic script, run for the global it defines
		await import('../../state-transfer.ts');
		const { details } = await import('../../src/lib/details.svelte.ts');
		await details.start();
		expect(details.count).toBe(0);
		expect(chrome.storage.onChanged.addListener).not.toHaveBeenCalled();
	});
});
