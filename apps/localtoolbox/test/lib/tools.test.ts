// src/lib/tools.ts — the registry the menu and the shell read.
import { describe, it, expect, vi } from 'vitest';
import { TOOLS, toolById, loadPanel } from '../../src/lib/tools.ts';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';
import { flush } from '../helpers/flush.ts';
import { SOUNDCLOUD_TAB } from '../helpers/panel.ts';

declare const chrome: ChromeMock;

describe('the registry', () => {
	it('describes four tools, each with what the menu needs to draw it', () => {
		expect(TOOLS).toHaveLength(4);

		for (const tool of TOOLS) {
			expect(tool.id).toMatch(/^[a-z-]+$/);
			expect(tool.name).toBeTruthy();
			expect(tool.short).toBeTruthy();
			expect(tool.description).toMatch(/\.$/);
			expect(tool.tone).toMatch(/^bg-[a-z]+-[a-z]+$/);
			expect(tool.load).toBeTypeOf('function');
		}

		expect(new Set(TOOLS.map((t) => t.id)).size).toBe(4);
	});

	it('loads each tool’s panel from its own chunk, once', async () => {
		for (const tool of TOOLS) {
			const panel = await loadPanel(tool.id);
			expect(panel, tool.id).toBeTypeOf('function');
			// A second ask is the same component, not a second chunk: going back to
			// the menu and into the same tool again loads nothing.
			expect(loadPanel(tool.id)).toBe(loadPanel(tool.id));
			await expect(loadPanel(tool.id)).resolves.toBe(panel);
		}
	});

	it('has nothing to load for an id it has no tool for', () => {
		expect(loadPanel('soundcloud-lyrics')).toBe(null);
		// The session queue was one of these entries until it moved into the
		// header, and a session left by that version names it: an old id has to
		// land on the menu rather than on a chunk that is not there.
		expect(loadPanel('session-queue')).toBe(null);
	});

	it('does not remember a chunk that failed to arrive', async () => {
		// A fresh copy of the registry: this one has its loader broken, and the
		// cache it fills must not be the one the other tests read.
		vi.resetModules();
		const fresh = await import('../../src/lib/tools.ts');
		const tool = fresh.TOOLS[0];
		const real = tool.load;
		tool.load = () => Promise.reject(new Error('chunk missing'));

		await expect(fresh.loadPanel(tool.id)).rejects.toThrow('chunk missing');

		// Nothing was cached, so opening the tool again really tries again.
		tool.load = real;
		expect(await fresh.loadPanel(tool.id)).toBeTypeOf('function');
	});

	// What opening a tool starts before its panel exists. The cheap read of the
	// page is all any of them may start: looking a whole page's tracks up on
	// SoundCloud waits for somebody to ask.
	it.each([
		['soundcloud-playlist-to-text', [[SOUNDCLOUD_TAB.id, { type: 'EXTRACT_TRACKS' }]]],
		['manager', [[SOUNDCLOUD_TAB.id, { type: 'EXTRACT_TRACKS' }]]],
		['third-party-downloads', []],
		['artists', [[SOUNDCLOUD_TAB.id, { type: 'EXTRACT_TRACKS' }]]]
	])('starts the slow thing %s does first, and nothing slower', async (id, asked) => {
		vi.resetModules();
		globalThis.chrome = createChrome({ tab: SOUNDCLOUD_TAB });
		const fresh = await import('../../src/lib/tools.ts');
		fresh.toolById(id)!.warm!();
		await flush();
		expect(chrome.tabs.query).toHaveBeenCalledTimes(1);
		expect(chrome.tabs.sendMessage.mock.calls).toEqual(asked);
	});

	it('finds a tool by id, and answers for one that is not there', () => {
		expect(toolById('manager')!.name).toBe('Manager');
		expect(toolById('nothing-like-it')).toBe(null);
	});
});
