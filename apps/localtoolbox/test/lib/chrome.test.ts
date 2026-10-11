// src/lib/chrome.ts — the browser APIs behind functions that never throw.
//
// What is here is the one part of it with state: a tab read started before
// anybody asked for it. Everything else (asking the page, opening a tab, the
// SoundCloud URL test) is covered where it shows, in the panels.
import { describe, it, expect, vi } from 'vitest';
import { createChrome, type ChromeMock, type TabStub } from '../helpers/chrome.ts';
import { SOUNDCLOUD_TAB } from '../helpers/panel.ts';

declare const chrome: ChromeMock;

async function fresh(tab: TabStub | null = SOUNDCLOUD_TAB) {
	vi.resetModules();
	globalThis.chrome = createChrome({ tab });

	return import('../../src/lib/chrome.ts');
}

describe('the active tab, read before it is asked for', () => {
	it('is handed to the next ask rather than read again', async () => {
		const { warmActiveTab, activeTab } = await fresh();
		warmActiveTab();
		expect(chrome.tabs.query).toHaveBeenCalledTimes(1);

		expect((await activeTab())!.id).toBe(SOUNDCLOUD_TAB.id);
		expect(chrome.tabs.query).toHaveBeenCalledTimes(1);
		// The stub answers whatever it is asked, so what is asked is the check:
		// the tab in front, in the window the popup belongs to.
		expect(chrome.tabs.query).toHaveBeenCalledWith({ active: true, currentWindow: true });
	});

	it('is one read, however many times it is started', async () => {
		const { warmActiveTab, activeTab } = await fresh();
		warmActiveTab();
		warmActiveTab();
		await activeTab();
		expect(chrome.tabs.query).toHaveBeenCalledTimes(1);
	});

	it('is taken once: the ask after it reads the tab again', async () => {
		const { warmActiveTab, activeTab } = await fresh();
		warmActiveTab();
		await activeTab();
		await activeTab();
		expect(chrome.tabs.query).toHaveBeenCalledTimes(2);
	});

	it('answers with null, rather than throwing, when there is no tab to read', async () => {
		const { warmActiveTab, activeTab } = await fresh(null);
		warmActiveTab();
		expect(await activeTab()).toBe(null);

		chrome.tabs.query.mockRejectedValue(new Error('no permission'));
		expect(await activeTab()).toBe(null);
	});
});

// A sign-in no fetch can walk: a tab is sent out through it and watched until
// it comes back. What the tab does in between is somebody else's site, and
// invisible here — only the landing is ever read.
describe('a tab sent out through a sign-in', () => {
	const TRIP = { landing: 'https://hypeddit.com/', timeout: 200, every: 1 };

	const where = (...urls: string[]) => {
		let at = 0;
		chrome.tabs.get.mockImplementation(async (id) => ({
			id,
			url: urls[Math.min(at++, urls.length - 1)]
		}));
	};

	it('says so when it comes back, and takes the tab away with it', async () => {
		const { roundTrip } = await fresh();
		where('https://accounts.spotify.com/authorize', 'https://hypeddit.com/spotify_callback?code=1');

		expect(await roundTrip('https://accounts.spotify.com/authorize', TRIP)).toBe(true);
		expect(chrome.tabs.create).toHaveBeenCalledWith(expect.objectContaining({ active: false }));
		expect(chrome.tabs.remove).toHaveBeenCalledWith(99);
	});

	it('gives up on a trip that stops somewhere to ask a question', async () => {
		const { roundTrip } = await fresh();
		where('https://accounts.spotify.com/login');

		expect(await roundTrip('https://accounts.spotify.com/authorize', TRIP)).toBe(false);
		// Still cleaned up: a tab nobody asked for is not left open behind the
		// popup.
		expect(chrome.tabs.remove).toHaveBeenCalledWith(99);
	});

	it('takes a tab closed under it, or one that never opened, as a no', async () => {
		const { roundTrip } = await fresh();
		chrome.tabs.get.mockRejectedValue(new Error('No tab with id'));
		expect(await roundTrip('https://accounts.spotify.com/authorize', TRIP)).toBe(false);
		expect(chrome.tabs.remove).not.toHaveBeenCalled();

		chrome.tabs.create.mockRejectedValue(new Error('no permission'));
		expect(await roundTrip('https://accounts.spotify.com/authorize', TRIP)).toBe(false);
	});

	it('waits out the part of the trip it is not allowed to see', async () => {
		const { roundTrip } = await fresh();
		// A tab on a host the extension has no permission for reports no URL at
		// all, which is the middle of every one of these trips.
		chrome.tabs.get.mockResolvedValue({ id: 99 });
		expect(await roundTrip('https://accounts.spotify.com/authorize', TRIP)).toBe(false);
	});

	it('reads a tab still loading by where it is going', async () => {
		const { roundTrip } = await fresh();
		chrome.tabs.get.mockResolvedValue({ id: 99, pendingUrl: 'https://hypeddit.com/one' });
		expect(await roundTrip('https://accounts.spotify.com/authorize', TRIP)).toBe(true);
	});
});
