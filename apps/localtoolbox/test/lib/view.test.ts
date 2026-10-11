// src/lib/view.svelte.ts — the place the popup was left in, kept for the
// browsing session.
//
// The store is a module singleton, so each test imports its own copy against
// its own chrome mock; `flush` covers the microtask the writes are batched on.
import { describe, it, expect, vi } from 'vitest';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';
import { flush } from '../helpers/flush.ts';

declare const chrome: ChromeMock;

/** A freshly imported store, started against `session` as what is stored. */
async function load(session = {}) {
	vi.resetModules();
	globalThis.chrome = createChrome({ session });
	const module = await import('../../src/lib/view.svelte.ts');
	await module.view.start();

	return module;
}

const stored = () => chrome.testStore('session').view;

describe('view', () => {
	it('opens on the tool menu when the session holds nothing', async () => {
		const { view } = await load();
		expect(view.ready).toBe(true);
		expect(view.open).toBe(null);
		expect(view.get('any-tool', 'query', 'fallback')).toBe('fallback');
	});

	it('comes back to the tool that was open, with what its panel had', async () => {
		const { view } = await load({
			view: { open: 'manager', panels: { manager: { query: 'bicep' } } }
		});

		expect(view.open).toBe('manager');
		expect(view.get('manager', 'query', '')).toBe('bicep');
		// A key the tool has never set, and a tool that has never kept anything.
		expect(view.get('manager', 'scroll', 0)).toBe(0);
		// A scope nothing has ever kept anything under — the queue was a tool
		// with a route of its own until it moved into the header.
		expect(view.get('session-queue', 'query', '')).toBe('');
	});

	it('writes the open tool and every panel value to session storage', async () => {
		const { view } = await load();
		view.open = 'third-party-downloads';
		view.set('third-party-downloads', 'scroll', 120);
		await flush();
		expect(stored()).toEqual({
			open: 'third-party-downloads',
			queue: false,
			settings: false,
			panels: { 'third-party-downloads': { scroll: 120 } }
		});

		view.open = null;
		await flush();
		expect(stored().open).toBe(null);
	});

	// The queue sheet is not a tool and has no route, so whether it is up is a
	// place of its own here rather than a value under some panel's name.
	it('keeps whether the queue sheet was up, and comes back to it', async () => {
		const { view } = await load();
		expect(view.queueOpen).toBe(false);

		view.queueOpen = true;
		await flush();
		expect(stored().queue).toBe(true);

		const reopened = await load({ view: { open: null, queue: true, panels: {} } });
		expect(reopened.view.queueOpen).toBe(true);
	});

	// The settings sheet is the other one, kept the same way — and the two are
	// exclusive: both come up over the whole popup from the same row of
	// buttons, so one opening puts the other away.
	it('keeps whether the settings sheet was up, and never both at once', async () => {
		const { view } = await load();
		expect(view.settingsOpen).toBe(false);

		view.settingsOpen = true;
		await flush();
		expect(stored()).toMatchObject({ settings: true, queue: false });

		view.queueOpen = true;
		await flush();
		expect(stored()).toMatchObject({ settings: false, queue: true });

		view.settingsOpen = true;
		await flush();
		expect(stored()).toMatchObject({ settings: true, queue: false });

		view.settingsOpen = false;
		await flush();
		expect(stored()).toMatchObject({ settings: false, queue: false });

		const reopened = await load({ view: { open: null, settings: true, panels: {} } });
		expect(reopened.view.settingsOpen).toBe(true);
	});

	// Nothing is drawn before the session has answered, the sheet least of all:
	// it would come up over a view that is still a ghost.
	it('says the sheet is shut until the session has been read', async () => {
		vi.resetModules();
		globalThis.chrome = createChrome({ session: { view: { open: null, queue: true, panels: {} } } });
		const { view } = await import('../../src/lib/view.svelte.ts');
		expect(view.ready).toBe(false);
		expect(view.queueOpen).toBe(false);

		await view.start();
		expect(view.queueOpen).toBe(true);
	});

	// A session left by the version that had the queue as a tool: there is no
	// `queue` in what it wrote, and the sheet has to start shut rather than
	// undefined.
	it('starts the sheet shut for a session written before it existed', async () => {
		const { view } = await load({ view: { open: 'manager', panels: {} } });
		expect(view.queueOpen).toBe(false);
	});

	// Everything that moves in one turn is one place, so a keystroke that
	// changes a search and the scroll under it costs one write, not two.
	it('writes once however many values moved in the same turn', async () => {
		const { view } = await load();
		view.open = 'manager';
		view.set('manager', 'query', 'bi');
		view.set('manager', 'query', 'bic');
		await flush();
		expect(chrome.storage.session.set).toHaveBeenCalledTimes(1);

		view.set('manager', 'query', 'bice');
		await flush();
		expect(chrome.storage.session.set).toHaveBeenCalledTimes(2);
	});

	it('keeps the panels of other tools when one of them is written', async () => {
		const { view } = await load({
			view: { open: null, panels: { 'third-party-downloads': { scroll: 40 } } }
		});

		view.set('manager', 'query', 'glue');
		await flush();
		expect(stored().panels).toEqual({
			'third-party-downloads': { scroll: 40 },
			manager: { query: 'glue' }
		});
	});

	it('is read once, however many times it is started', async () => {
		const { view } = await load({ view: { open: 'third-party-downloads', panels: {} } });
		chrome.storage.session.get.mockClear();
		await view.start();
		expect(chrome.storage.session.get).not.toHaveBeenCalled();
		expect(view.open).toBe('third-party-downloads');
	});

	it('opens on the menu when the session cannot be read, and holds its place anyway', async () => {
		vi.resetModules();
		globalThis.chrome = createChrome();
		chrome.storage.session.get.mockRejectedValue(new Error('no session storage'));
		chrome.storage.session.set.mockRejectedValue(new Error('no session storage'));
		const { view, remembered } = await import('../../src/lib/view.svelte.ts');
		await view.start();

		expect(view.ready).toBe(true);
		expect(view.open).toBe(null);

		const query = remembered('third-party-downloads', 'query', '');
		query.value = 'glue';
		await flush();
		// The popup still shows what it holds; only outliving it is lost.
		expect(query.value).toBe('glue');
		expect(view.get('third-party-downloads', 'query', '')).toBe('glue');
	});

	// An older session could have been written by a version that kept less.
	it('copes with a stored place that names neither a tool nor any panel', async () => {
		const { view } = await load({ view: {} });
		expect(view.open).toBe(null);
		expect(view.get('third-party-downloads', 'query', 'none')).toBe('none');
	});
});

describe('remembered', () => {
	it('starts from what the tool had, and writes every change under its name', async () => {
		const { remembered } = await load({
			view: { open: null, panels: { 'third-party-downloads': { drawer: true } } }
		});

		const drawer = remembered('third-party-downloads', 'drawer', false);
		expect(drawer.value).toBe(true);

		drawer.value = false;
		await flush();
		expect(drawer.value).toBe(false);
		expect(stored().panels['third-party-downloads'].drawer).toBe(false);
	});

	it('starts from the default when the tool has nothing under that name', async () => {
		const { remembered } = await load();
		expect(remembered('third-party-downloads', 'paste', '').value).toBe('');
		expect(remembered('third-party-downloads', 'drawer', false).value).toBe(false);
	});
});
