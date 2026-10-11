// src/lib/settings.svelte.ts — the extension's own settings, and the one
// thing that makes them different from a panel's options: they live in
// chrome.storage.local, because the content script has to be able to read
// them and cannot see the popup page's localStorage.
//
// What the sheet built on them looks like is test/settings-sheet.test.ts;
// this is the store on its own.
import { describe, it, expect, vi } from 'vitest';
import { flush } from '../helpers/flush.ts';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

// A fresh module graph each time, so the singleton starts empty.
async function freshSettings(storage = {}) {
	vi.resetModules();
	globalThis.chrome = createChrome({ storage });
	const module = await import('../../src/lib/settings.svelte.ts');
	await module.settings.start();

	return module;
}

describe('the settings', () => {
	it('read what is stored', async () => {
		const { settings } = await freshSettings({ settings: { replaceDownloadButtons: false } });
		expect(settings.replaceDownloadButtons).toBe(false);
	});

	it('are read once, however many views ask', async () => {
		const { settings } = await freshSettings();
		expect(chrome.storage.local.get).toHaveBeenCalledTimes(1);
		await settings.start();
		expect(chrome.storage.local.get).toHaveBeenCalledTimes(1);
		expect(chrome.storage.onChanged.addListener).toHaveBeenCalledTimes(1);
	});

	// Stored settings are not this version's to trust: an older install, a
	// later one, or a hand-edited storage area can hold anything.
	it.each([
		['nothing stored', {}],
		['a value of the wrong type', { settings: { replaceDownloadButtons: 'yes' } }],
		['a value of no type at all', { settings: { replaceDownloadButtons: null } }],
		['something that is not an object', { settings: 'on' }],
		['null', { settings: null }],
		['a setting this version has never heard of', { settings: { somethingElse: 1 } }]
	])('fall back to the default given %s', async (_what, storage) => {
		const { settings, DEFAULTS } = await freshSettings(storage);
		expect(settings.replaceDownloadButtons).toBe(DEFAULTS.replaceDownloadButtons);
	});

	it('write the whole object back, under the key the content script reads', async () => {
		const { settings } = await freshSettings();
		settings.replaceDownloadButtons = false;
		await flush();
		expect(chrome.testStore().settings).toEqual({
			replaceDownloadButtons: false,
			likesListView: false
		});
		expect(settings.replaceDownloadButtons).toBe(false);
	});

	// Anything a switch can hand over is a flag by the time it is stored: the
	// page reads a boolean or falls back, so writing anything else would be
	// writing the default in disguise.
	it('store what a switch gives as a flag', async () => {
		const { settings } = await freshSettings();
		// @ts-expect-error -- a 0 where a switch would hand over false
		settings.replaceDownloadButtons = 0;
		await flush();
		expect(chrome.testStore().settings).toEqual({
			replaceDownloadButtons: false,
			likesListView: false
		});
	});

	it('follow a change made in another window, and ignore other areas', async () => {
		const { settings } = await freshSettings();
		chrome.testEmitStorageChange({ settings: { newValue: { replaceDownloadButtons: false } } });
		await flush();
		expect(settings.replaceDownloadButtons).toBe(false);

		chrome.testEmitStorageChange({ settings: { newValue: { replaceDownloadButtons: true } } }, 'session');
		chrome.testEmitStorageChange({ downloaded: { newValue: {} } });
		await flush();
		expect(settings.replaceDownloadButtons).toBe(false);
	});

	it('hold the defaults when storage cannot be read, and write nothing over it', async () => {
		vi.resetModules();
		globalThis.chrome = createChrome();
		chrome.storage.local.get.mockRejectedValue(new Error('storage unavailable'));
		const { settings, DEFAULTS } = await import('../../src/lib/settings.svelte.ts');
		await settings.start();
		expect(settings.replaceDownloadButtons).toBe(DEFAULTS.replaceDownloadButtons);
		// Nothing was read, so nothing is followed either.
		expect(chrome.storage.onChanged.addListener).not.toHaveBeenCalled();
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});

	it('keep showing the new value when the write is refused', async () => {
		const { settings } = await freshSettings();
		chrome.storage.local.set.mockRejectedValue(new Error('QUOTA_BYTES quota exceeded'));
		settings.replaceDownloadButtons = false;
		await flush();
		expect(settings.replaceDownloadButtons).toBe(false);
	});
});
