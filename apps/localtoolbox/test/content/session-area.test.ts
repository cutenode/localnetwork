// content.ts — the session area opening after the page has loaded.
//
// Chrome shuts chrome.storage.session to content scripts until a trusted
// context opens it, and the popup is what does that (src/lib/queue.svelte.ts).
// After a browser restart that is the ordinary order of things: a SoundCloud
// tab is already open, and has found the area shut, before the popup is first
// opened. The page offers no queue button while it cannot write the queue, and
// offers one as soon as the popup opens the door — without a reload.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import { loadContentScript, disposeContentScripts } from '../helpers/load.ts';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';
import { flush } from '../helpers/flush.ts';
import { render, playlistPage, playlistRow } from '../helpers/dom.ts';

declare const chrome: ChromeMock;

beforeEach(() => {
	vi.useFakeTimers();
});

afterEach(disposeContentScripts);

const OPEN = { accessLevel: 'TRUSTED_AND_UNTRUSTED_CONTEXTS' } as const;

const page = () =>
	playlistPage({
		rows: ['/a/one', '/a/two'].map((path) => playlistRow({ href: `${path}?in=c/sets/p` }))
	});

const buttonOf = (permalink: string) =>
	document
		.querySelector(`a[href^="${permalink}"]`)!
		.closest('.trackItem')!
		.querySelector<HTMLElement>('.sc-artwork > .tbx-queue')!;

// What the popup does as its queue starts, and then as it queues something.
const popupOpensAndQueues = async (permalinks: string[]) => {
	await chrome.storage.session.setAccessLevel(OPEN);
	await chrome.storage.session.set({ queue: permalinks.map((permalink) => ({ permalink })) });
	await flush();
};

describe('a session area the popup opens after the page loaded', () => {
	// The rest of this file stands on the stub doing what Chrome does.
	it('is shut to the page until the popup opens it', async () => {
		globalThis.chrome = createChrome({ sessionOpen: false });
		await expect(chrome.storage.session.get('queue')).rejects.toThrow('not allowed');
		await expect(chrome.storage.session.set({ queue: [] })).rejects.toThrow('not allowed');

		await chrome.storage.session.setAccessLevel(OPEN);
		await chrome.storage.session.set({ queue: [] });
		await expect(chrome.storage.session.get('queue')).resolves.toEqual({ queue: [] });
	});

	it('offers the queue button once the popup opens it, without a reload', async () => {
		globalThis.chrome = createChrome({ sessionOpen: false });
		render(page(), '/c/sets/p');
		await loadContentScript();
		// Shut: a button there could not be written anywhere.
		expect(document.querySelectorAll('.tbx-queue')).toHaveLength(0);

		await popupOpensAndQueues(['/a/one']);

		expect(buttonOf('/a/one').dataset.state).toBe('small:yes');
		expect(buttonOf('/a/two').dataset.state).toBe('small:no');
	});

	it('queues from the page into the area the popup opened', async () => {
		globalThis.chrome = createChrome({ sessionOpen: false });
		render(page(), '/c/sets/p');
		await loadContentScript();
		await popupOpensAndQueues(['/a/one']);

		buttonOf('/a/two').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
		await flush();

		expect(chrome.testStore('session').queue.map((entry) => entry.permalink)).toEqual(['/a/one', '/a/two']);
		expect(buttonOf('/a/two').dataset.state).toBe('small:yes');
	});
});
