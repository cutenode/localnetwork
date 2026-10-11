// src/lib/components/QueueSheet.svelte and the header button that opens it.
//
// Not under test/panels/, because this is not a panel: the queue is reached
// from the header of every view rather than from the tool menu, and the shell
// draws it over whichever tool is open. So every test here mounts the whole
// popup, as someone using it would have it.
import { describe, it, expect, vi } from 'vitest';
import {
	button,
	buttons,
	byLabel,
	click,
	mountApp,
	openQueue,
	panelChrome,
	queueButton,
	queueSheet,
	rows,
	settle,
	text,
	track,
	type Target
} from './helpers/panel.ts';
import type { ChromeMock } from './helpers/chrome.ts';

declare const chrome: ChromeMock;

const ISO = '2026-09-08T12:00:00.000Z';

const QUEUED = [track('Artist A', 'One'), track('Artist B', 'Two'), track('Artist C', 'Three')];

const queued = (tracks = QUEUED) => ({ queue: tracks });

// A tab that is not SoundCloud at all: the queue is not the page, and nothing
// here should depend on what is in front.
const OTHER_TAB = { id: 1, url: 'https://example.com/' };

const openApp = async (options = {}) => {
	panelChrome({ tab: OTHER_TAB, ...options });

	return mountApp();
};

const meta = () => queueSheet()?.querySelector('header p')?.textContent.trim() ?? '';

const permalinks = () => rows().map((li) => li.dataset.permalink);

const clean = (node: Target) => node!.textContent.replace(/\s+/g, ' ').trim();

const rowFor = (permalink: string) => document.querySelector(`li[data-permalink="${permalink}"]`);

const markButton = (permalink: string) => rowFor(permalink)!.querySelector('button[aria-label="mark downloaded"]');

const removeButton = (permalink: string) =>
	rowFor(permalink)!.querySelector('button[aria-label="remove from the queue"]');

const isMarked = (permalink: string) => markButton(permalink)!.getAttribute('aria-pressed') === 'true';

const markAll = () => button('downloaded');

const removeMarked = () => buttons().find((b) => clean(b).endsWith('marked out'));

const clearButton = () =>
	[...queueSheet()!.querySelectorAll('header button')].find((b) => /clear|empty/.test(clean(b)));

const sendButton = (label: string) => buttons().find((b) => clean(b) === label);

const heading = () => document.querySelector('h1')!.textContent.trim();

describe('the button in the header', () => {
	it('is on the tool menu and in every tool, and says how full the queue is', async () => {
		await openApp({ session: queued() });
		expect(queueButton()!.getAttribute('aria-label')).toBe('session queue: 3 tracks');
		expect(clean(queueButton())).toBe('3');

		// Into a tool, and the same button is still there.
		await click(button('playlist to text'));
		expect(heading()).toBe('SoundCloud playlist to text');
		expect(queueButton()!.getAttribute('aria-label')).toBe('session queue: 3 tracks');
	});

	it('says the queue is empty rather than hiding itself', async () => {
		await openApp();
		expect(queueButton()!.getAttribute('aria-label')).toBe('session queue: empty');
		// Nothing to count, so there is no count.
		expect(clean(queueButton())).toBe('');
	});

	it('is the one gesture: it opens the sheet and closes it again', async () => {
		await openApp({ session: queued() });
		expect(queueSheet()).toBe(null);
		expect(queueButton()!.getAttribute('aria-expanded')).toBe('false');

		await click(queueButton());
		expect(queueSheet()).not.toBe(null);
		expect(queueButton()!.getAttribute('aria-expanded')).toBe('true');

		await click(queueButton());
		expect(queueSheet()).toBe(null);
	});

	it('follows the queue as it is filled and emptied', async () => {
		await openApp();
		expect(clean(queueButton())).toBe('');

		const { queue } = await import('../src/lib/queue.svelte.ts');
		await queue.add([track('Bicep', 'Glue')]);
		await settle();
		expect(clean(queueButton())).toBe('1');
		expect(queueButton()!.getAttribute('aria-label')).toBe('session queue: 1 track');

		await queue.clear();
		await settle();
		expect(clean(queueButton())).toBe('');
		expect(queueButton()!.getAttribute('aria-label')).toBe('session queue: empty');
	});
});

describe('an empty queue', () => {
	it('says where tracks come from', async () => {
		await openApp();
		await openQueue();
		expect(text()).toMatch(/nothing here yet/);
		expect(text()).toMatch(/queue tracks from Manager/);
		expect(rows()).toHaveLength(0);
		expect(meta()).toBe('nothing in it yet');
		// Nothing to do to a queue with nothing in it, but there is a way out.
		expect(clearButton()).toBeUndefined();
		expect(markAll()).toBeUndefined();
		expect(byLabel('close the queue')).not.toBe(null);
	});
});

describe('the listed tracks', () => {
	it('lists what was queued, in the order it was queued', async () => {
		await openApp({ session: queued() });
		await openQueue();
		expect(permalinks()).toEqual(['/artista/one', '/artistb/two', '/artistc/three']);
		expect(rows()[0].textContent).toMatch(/One/);
		expect(meta()).toBe('3 tracks');
	});

	it('says how many of them are already marked as downloaded', async () => {
		await openApp({ session: queued(), storage: { downloaded: { '/artistb/two': ISO } } });
		await openQueue();
		expect(meta()).toBe('3 tracks · 1 marked');
		expect(isMarked('/artistb/two')).toBe(true);
		expect(isMarked('/artista/one')).toBe(false);
		expect(clean(markAll())).toBe('mark 2 tracks downloaded');
	});

	it('marks and unmarks one track by hand', async () => {
		await openApp({ session: queued() });
		await openQueue();
		await click(markButton('/artista/one'));
		expect(chrome.testStore().downloaded).toEqual({ '/artista/one': ISO });
		expect(isMarked('/artista/one')).toBe(true);

		await click(markButton('/artista/one'));
		expect(chrome.testStore().downloaded).toEqual({});
		expect(isMarked('/artista/one')).toBe(false);
	});

	it('takes one out, leaving its mark alone', async () => {
		await openApp({ session: queued(), storage: { downloaded: { '/artistb/two': ISO } } });
		await openQueue();
		await click(removeButton('/artistb/two'));
		expect(permalinks()).toEqual(['/artista/one', '/artistc/three']);
		expect(chrome.testStore('session').queue.map((item) => item.permalink)).toEqual(['/artista/one', '/artistc/three']);
		expect(chrome.testStore().downloaded).toEqual({ '/artistb/two': ISO });
	});
});

describe('marking the batch', () => {
	it('marks every track that is not marked yet, in one write', async () => {
		await openApp({ session: queued(), storage: { downloaded: { '/artistb/two': ISO } } });
		await openQueue();
		await click(markAll());
		expect(chrome.testStore().downloaded).toEqual({
			'/artista/one': ISO,
			'/artistb/two': ISO,
			'/artistc/three': ISO
		});
		expect(text()).toMatch(/marked 2 tracks downloaded/);
		// The tracks stay listed, marked, until the queue is cleared.
		expect(permalinks()).toHaveLength(3);
		expect(clean(markAll())).toBe('all marked downloaded');
		expect(markAll()!.disabled).toBe(true);
	});

	// The marks are the one thing here meant to outlive the session, so a write
	// that did not happen says so rather than reporting three tracks marked.
	it('says so when storage refuses the write, and claims nothing', async () => {
		await openApp({ session: queued() });
		await openQueue();
		chrome.storage.local.set.mockRejectedValue(new Error('QUOTA_BYTES quota exceeded'));

		await click(markAll());
		expect(text()).toMatch(/couldn’t save the marks/);
		expect(text()).not.toMatch(/marked 3 tracks downloaded/);
		expect(isMarked('/artista/one')).toBe(false);
	});
});

describe('taking the marked ones out', () => {
	it('is not offered while nothing in the queue is marked', async () => {
		await openApp({ session: queued() });
		await openQueue();
		expect(removeMarked()).toBeUndefined();

		// Mark one by hand and it turns up, counting only that one.
		await click(markButton('/artistb/two'));
		expect(clean(removeMarked())).toBe('take 1 track marked out');
	});

	it('takes every marked track out in one write, leaving the marks', async () => {
		await openApp({
			session: queued(),
			storage: { downloaded: { '/artista/one': ISO, '/artistc/three': ISO } }
		});
		await openQueue();
		expect(clean(removeMarked())).toBe('take 2 tracks marked out');

		await click(removeMarked());
		expect(permalinks()).toEqual(['/artistb/two']);
		expect(chrome.testStore('session').queue.map((item) => item.permalink)).toEqual(['/artistb/two']);
		// The marks are what a worked-through queue leaves behind: they stay.
		expect(chrome.testStore().downloaded).toEqual({ '/artista/one': ISO, '/artistc/three': ISO });
		expect(text()).toMatch(/took 2 tracks out of the queue/);
		// Nothing marked is left, so there is nothing left for it to take.
		expect(removeMarked()).toBeUndefined();
		expect(meta()).toBe('1 track');
	});

	// "Clear" empties the lot and asks twice for it; this empties the same queue
	// only because every track in it is done with, and asks nothing.
	it('empties the queue when the whole batch is marked, without asking twice', async () => {
		await openApp({ session: queued() });
		await openQueue();
		await click(markAll());
		expect(clean(removeMarked())).toBe('take all marked out');

		await click(removeMarked());
		expect(rows()).toHaveLength(0);
		expect(chrome.testStore('session').queue).toEqual([]);
		expect(Object.keys(chrome.testStore().downloaded)).toHaveLength(3);
		expect(meta()).toBe('nothing in it yet');
		// Emptied, not closed — as clearing it is.
		expect(queueSheet()).not.toBe(null);
	});

	it('leaves a track marked on the SoundCloud page but never queued alone', async () => {
		await openApp({ session: queued(), storage: { downloaded: { '/artistb/two': ISO } } });
		await openQueue();
		await click(removeMarked());
		expect(permalinks()).toEqual(['/artista/one', '/artistc/three']);
		expect(clean(markAll())).toBe('mark 2 tracks downloaded');
	});
});

describe('clearing the queue', () => {
	it('asks once more, then empties it, and leaves the marks', async () => {
		await openApp({ session: queued(), storage: { downloaded: { '/artista/one': ISO } } });
		await openQueue();
		expect(clean(clearButton())).toBe('clear');

		await click(clearButton());
		expect(clean(clearButton())).toBe('empty it?');
		expect(rows()).toHaveLength(3);

		await click(clearButton());
		expect(rows()).toHaveLength(0);
		expect(chrome.testStore('session').queue).toEqual([]);
		expect(chrome.testStore().downloaded).toEqual({ '/artista/one': ISO });
		expect(text()).toMatch(/queue emptied/);
		// Emptied, not closed: the sheet stays up and says so.
		expect(queueSheet()).not.toBe(null);
		expect(meta()).toBe('nothing in it yet');
	});

	// The view store's rule for a confirm button, which the sheet meets by being
	// built again each time it is opened.
	it('comes back disarmed when the sheet is closed and opened again', async () => {
		await openApp({ session: queued() });
		await openQueue();
		await click(clearButton());
		expect(clean(clearButton())).toBe('empty it?');

		await click(byLabel('close the queue'));
		await openQueue();
		expect(clean(clearButton())).toBe('clear');
		expect(rows()).toHaveLength(3);
	});

	it('goes back to asking when the second click does not come', async () => {
		vi.useFakeTimers();
		await openApp({ session: queued() });
		await openQueue();
		await click(clearButton());
		expect(clean(clearButton())).toBe('empty it?');

		await vi.advanceTimersByTimeAsync(4000);
		await settle();
		expect(clean(clearButton())).toBe('clear');
		expect(rows()).toHaveLength(3);
	});
});

describe('putting it away', () => {
	it('closes on the close button, leaving the view under it where it was', async () => {
		await openApp({ session: queued() });
		await click(button('Manager'));
		expect(heading()).toBe('Manager');

		await openQueue();
		expect(queueSheet()).not.toBe(null);

		await click(byLabel('close the queue'));
		expect(queueSheet()).toBe(null);
		expect(heading()).toBe('Manager');
	});

	it('closes on Escape', async () => {
		await openApp({ session: queued() });
		await openQueue();

		window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
		await settle();
		expect(queueSheet()).toBe(null);
	});

	it('stays up for every other key', async () => {
		await openApp({ session: queued() });
		await openQueue();

		window.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', bubbles: true }));
		await settle();
		expect(queueSheet()).not.toBe(null);
	});

	it('shuts the view behind it away from the keyboard while it is up', async () => {
		await openApp({ session: queued() });
		const main = document.querySelector('main')!;
		expect(main.hasAttribute('inert')).toBe(false);

		await openQueue();
		expect(main.hasAttribute('inert')).toBe(true);

		await click(byLabel('close the queue'));
		expect(main.hasAttribute('inert')).toBe(false);
	});
});

describe('handing the batch to another tool', () => {
	it('opens that tool with the queue as its source, and goes away behind it', async () => {
		await openApp({ session: queued() });
		await openQueue();

		await click(sendButton('downloader'));
		expect(localStorage.getItem('pref:source')).toBe('queue');
		expect(heading()).toBe('third-party downloader');
		// The sheet was a way of getting there, not somewhere to come back to.
		expect(queueSheet()).toBe(null);
	});

	it('sends it to the text tool, which reads the queue rather than the tab', async () => {
		await openApp({ session: queued() });
		await openQueue();

		await click(sendButton('playlist to text'));
		expect(heading()).toBe('SoundCloud playlist to text');
		expect(localStorage.getItem('pref:source')).toBe('queue');
		expect(document.querySelector('textarea')!.value).toMatch(/One/);
		// The tab is example.com, and nothing asked it for a page.
		expect(text()).not.toMatch(/open a SoundCloud playlist, likes or track page/);
	});
});

describe('where the queue was scrolled to', () => {
	const body = () => queueSheet()!.querySelector('.scroll-slim')!;

	// The queue is read before anything is drawn, so unlike a panel's list the
	// rows are all there on the first turn and the place can go straight back.
	it('is where the list starts the next time the sheet is opened', async () => {
		await openApp({ session: queued() });
		await openQueue();
		body().scrollTop = 120;
		body().dispatchEvent(new Event('scroll'));
		await settle();

		await click(byLabel('close the queue'));
		await openQueue();
		expect(body().scrollTop).toBe(120);
	});

	it('starts at the top in a new browsing session', async () => {
		await openApp({ session: queued() });
		await openQueue();
		body().scrollTop = 120;
		body().dispatchEvent(new Event('scroll'));
		await settle();

		// A new session: the browser was closed, so its session storage went too.
		await openApp({ session: queued() });
		await openQueue();
		expect(body().scrollTop).toBe(0);
	});
});

describe('where the sheet was left', () => {
	it('comes back up when the popup is opened again on the same session', async () => {
		await openApp({ session: queued() });
		await openQueue();
		expect(chrome.testStore('session').view.queue).toBe(true);

		// What every click that opens a tab does: the popup is destroyed and made
		// again against the same browsing session.
		document.body.innerHTML = '';
		await mountApp();
		expect(queueSheet()).not.toBe(null);
		expect(permalinks()).toHaveLength(3);
	});

	it('stays away when it was closed', async () => {
		await openApp({ session: queued() });
		await openQueue();
		await click(byLabel('close the queue'));
		expect(chrome.testStore('session').view.queue).toBe(false);

		document.body.innerHTML = '';
		await mountApp();
		expect(queueSheet()).toBe(null);
	});
});
