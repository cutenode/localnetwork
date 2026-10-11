// src/lib/soundcloud.ts — reading the active tab's page, and starting that
// read before the panel that wants it exists.
//
// The interpretation of an answer is covered where it shows: each panel's
// tests say what the page's tracks, or the sentence explaining their absence,
// look like on screen. What is here is the handoff — the shell starts a read
// (see `warm` in $lib/tools.ts), the panel asks for one, and between them
// exactly one question reaches the page.
import { describe, it, expect, vi } from 'vitest';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';
import { extraction, SOUNDCLOUD_TAB, track } from '../helpers/panel.ts';

declare const chrome: ChromeMock;

/** A fresh copy of the module against a chrome whose page answers `response`. */
async function fresh({ tab = SOUNDCLOUD_TAB, response = extraction() } = {}) {
	vi.resetModules();
	globalThis.chrome = createChrome({ tab });
	chrome.tabs.sendMessage.mockResolvedValue(response);

	return import('../../src/lib/soundcloud.ts');
}

describe('a read started before the panel that wants it', () => {
	it('is the read the panel gets, and the page is asked once', async () => {
		const { warmPage, readPage } = await fresh({
			response: extraction({ tracks: [track('Bicep', 'Glue')] })
		});

		warmPage('EXTRACT_TRACKS');
		// Under way already: the tab has been asked for before anyone read.
		expect(chrome.tabs.query).toHaveBeenCalledTimes(1);

		const { data } = await readPage('EXTRACT_TRACKS');
		expect(data!.tracks).toHaveLength(1);
		expect(chrome.tabs.query).toHaveBeenCalledTimes(1);
		expect(chrome.tabs.sendMessage).toHaveBeenCalledTimes(1);
		expect(chrome.tabs.sendMessage).toHaveBeenCalledWith(SOUNDCLOUD_TAB.id, {
			type: 'EXTRACT_TRACKS'
		});
	});

	it('is taken once: a second read of the same page is a fresh one', async () => {
		const { warmPage, readPage } = await fresh();
		warmPage('EXTRACT_TRACKS');
		await readPage('EXTRACT_TRACKS');
		await readPage('EXTRACT_TRACKS');
		expect(chrome.tabs.sendMessage).toHaveBeenCalledTimes(2);
	});

	it('is not handed to a panel asking the page something else', async () => {
		const { warmPage, readPage } = await fresh();
		warmPage('EXTRACT_TRACKS');
		await readPage('GET_DOWNLOAD_DETAILS');
		expect(chrome.tabs.sendMessage).toHaveBeenLastCalledWith(SOUNDCLOUD_TAB.id, {
			type: 'GET_DOWNLOAD_DETAILS'
		});
	});

	it('is started once, however many times the shell asks for one', async () => {
		const { warmPage, readPage } = await fresh();
		warmPage('EXTRACT_TRACKS');
		warmPage('EXTRACT_TRACKS');
		await readPage('EXTRACT_TRACKS');
		// The tab lookup is shared anyway; what matters is that the page itself
		// is asked once, not once per warm.
		expect(chrome.tabs.sendMessage).toHaveBeenCalledTimes(1);
	});

	it('says what a read says when the tab is not a SoundCloud page', async () => {
		const { warmPage, readPage, OPEN_A_PAGE } = await fresh({
			tab: { id: 3, url: 'https://example.com/' }
		});

		warmPage('EXTRACT_TRACKS');
		expect((await readPage('EXTRACT_TRACKS')).status).toBe(OPEN_A_PAGE);
		expect(chrome.tabs.sendMessage).not.toHaveBeenCalled();
	});
});
