// src/lib/globals.ts: putting state-transfer.js on the popup page when the
// settings sheet asks for it, rather than in popup.html for every opening.
//
// jsdom fetches no scripts, so each test plays the browser's part: it finds
// the tag on the page and fires the event Chrome would.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { loadState } from '../helpers/load.ts';

const added = () => [...document.head.querySelectorAll<HTMLScriptElement>('script[src="./state-transfer.js"]')];

// A fresh module each time: what it remembers about the script is the thing under test.
const load = async () => {
	vi.resetModules();

	return (await import('../../src/lib/globals.ts')).loadTransfer;
};

beforeEach(() => {
	for (const script of added()) script.remove();
});

describe('loading the state file’s script', () => {
	it('adds it once, however often it is asked for, and answers when it has run', async () => {
		const loadTransfer = await load();
		const first = loadTransfer();
		const second = loadTransfer();
		expect(added()).toHaveLength(1);

		added()[0].dispatchEvent(new Event('load'));
		await expect(first).resolves.toBeUndefined();
		await expect(second).resolves.toBeUndefined();
		await loadTransfer();
		expect(added()).toHaveLength(1);
	});

	it('adds nothing when the page already has it', async () => {
		await loadState();
		const loadTransfer = await load();

		await expect(loadTransfer()).resolves.toBeUndefined();
		expect(added()).toEqual([]);
	});

	it('tries again after a script that failed to load', async () => {
		const loadTransfer = await load();
		const failed = loadTransfer();
		added()[0].dispatchEvent(new Event('error'));

		await expect(failed).rejects.toThrow('state-transfer.js did not load');
		// The broken tag goes, so the retry is the only one on the page.
		expect(added()).toEqual([]);

		const retry = loadTransfer();
		expect(added()).toHaveLength(1);
		added()[0].dispatchEvent(new Event('load'));
		await expect(retry).resolves.toBeUndefined();
	});
});
