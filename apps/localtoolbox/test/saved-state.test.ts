// src/lib/components/StateTransfer.svelte — the settings sheet's last
// section: everything the extension has saved, out to a JSON file and back.
//
// It was the Manager panel's footer, and these tests were in
// test/panels/manager.test.ts. The file carries four other tools' worth of
// state — the marks a SoundCloud page paints, the content script's cache of
// lookups, the session queue behind every header's count, three panels'
// options — so it is about the extension rather than about the one tool whose
// footer happened to hold it. Which means every test here mounts the whole
// popup and opens the sheet, as somebody using it would.
//
// What is most worth holding down is the promise the section makes: an import
// **adds**. Every part of it does — marks merge, the newer lookups win, listed
// and queued tracks go on the end without landing twice, and an option already
// chosen is left alone — and the one way to the other behaviour is a switch
// that starts off every time the popup opens.
import { describe, it, expect, beforeEach, vi, type Mock } from 'vitest';
import { createValidator, explain } from '../scripts/validate-state.mts';
import { MAX as ARTISTS_MAX } from '../src/tools/artists/artists.svelte.ts';
import { MAX as LISTED_MAX } from '../src/tools/manager/listed.svelte.ts';
import { audit } from './helpers/a11y.ts';
import { artistProfile, type FieldValue } from './helpers/state-fixtures.ts';
import {
	button,
	buttons,
	click,
	mountApp,
	openSettings,
	panelChrome,
	settle,
	settingsSheet,
	text,
	type
} from './helpers/panel.ts';
import type { ChromeMock } from './helpers/chrome.ts';

declare const chrome: ChromeMock;

// Not SoundCloud at all: restoring a backup is something one does *before*
// opening a playlist, and the section works wherever the popup is opened.
const OTHER_TAB = { id: 1, url: 'https://example.com/' };

const ISO = '2026-09-08T12:00:00.000Z';

const openApp = async (options = {}) => {
	panelChrome({ tab: OTHER_TAB, ...options });
	await mountApp();

	return openSettings();
};

const clean = (node: Element | null | undefined) => node!.textContent.replace(/\s+/g, ' ').trim();

// --- reading the section -----------------------------------------------------

const section = () =>
	[...settingsSheet()!.querySelectorAll('section')].find((one) =>
		one.querySelector('h3')?.textContent.includes('your data')
	);

/** The strip of counts: what there is to lose, as `{label: number}`. */
const held = () =>
	Object.fromEntries(
		[...section()!.querySelectorAll('dl > div')].map((one) => [
			clean(one.querySelector('dt')),
			Number(clean(one.querySelector('dd')))
		])
	);

/** The section's own note, which is where an export or an import reports. */
const note = () => clean(section()!.querySelector(':scope > p:last-child'));

const pasteBox = () => document.querySelector('textarea');

const fileInput = () => document.querySelector<HTMLInputElement>('input[type="file"]');

const replaceSwitch = () => document.getElementById('replace-state');

const replacing = () => replaceSwitch()!.getAttribute('aria-checked') === 'true';

// "import…" opens the drawer; the button inside it does the importing, and
// says which of the two kinds of import it would be.
const importNow = () => buttons().find((b) => ['add to what’s here', 'replace everything'].includes(clean(b)));

/** What the box says about the text in it, if it says anything. */
const verdict = () => {
	const status = section()!.querySelector('[role="status"], [role="alert"]');

	return status ? clean(status) : null;
};

const openImport = async () => {
	await click(button('import…'));

	return pasteBox();
};

// --- fixtures ----------------------------------------------------------------

const detailsEntry = (permalink = '/a/one', extra = {}) => ({
	id: 1,
	permalink,
	downloadable: true,
	hasDownloadsLeft: true,
	purchaseUrl: '',
	purchaseTitle: '',
	at: 1757332800000,
	...extra
});

const STATE = {
	format: 'toolbox.manager.state',
	version: 1,
	exportedAt: ISO,
	downloaded: { '/a/one': ISO },
	details: { '/a/one': detailsEntry() },
	listed: [],
	artists: [],
	queue: [],
	prefs: { background: false, sort: 'page', hideDownloaded: true }
};

/** A queued track, as the queue keeps one. */
const queueEntry = (permalink: string, extra = {}) => ({
	permalink,
	title: 'One',
	artist: 'A',
	artworkUrl: 'https://i1.sndcdn.com/artworks-abc-t120x120.jpg',
	details: null,
	...extra
});

/** A listed track, as the Manager's list keeps one. */
const listedEntry = (permalink: string, extra = {}) => ({
	...queueEntry(permalink),
	liked: null,
	reposted: null,
	domPurchaseTitle: '',
	from: 'My Playlist',
	at: 1757332800000,
	...extra
});

/** An artist, as the artists tool's list keeps one. */
const artistEntry = (permalink: string, extra = {}) => ({
	permalink,
	name: 'A',
	tracks: [],
	profile: null,
	from: 'My Playlist',
	at: 1757332800000,
	...extra
});

const file = (extra = {}) => ({ format: STATE.format, version: 1, ...extra });

// The schema, run the way anyone checking a backup would run it
// (scripts/validate-state.mts): what the sheet writes has to be what
// docs/manager-state.schema.json says a state file is.
const validate = createValidator();

const expectValid = (doc: FieldValue) => {
	const ok = validate(doc);
	expect(ok, explain(validate.errors).join('\n')).toBe(true);
};

// jsdom has no object URLs and would try to follow a download link, so both
// are stubbed; `saved` reports what the sheet offered to save.
const saved: { name: string; href: string }[] = [];

// What the sheet hands for a URL: always the file's blob.
let createObjectURL: Mock<(obj: Blob) => string>;

beforeEach(() => {
	saved.length = 0;
	let next = 0;
	createObjectURL = vi.fn<(obj: Blob) => string>(() => `blob:test/${next++}`);
	URL.createObjectURL = createObjectURL;
	URL.revokeObjectURL = vi.fn<typeof URL.revokeObjectURL>();
	vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
		saved.push({ name: this.download, href: this.href });
	});
});

// --- what is here ------------------------------------------------------------

describe('what there is to lose', () => {
	// An "export" button on its own says nothing about what would come out of
	// it. The numbers are the argument for pressing it.
	it('counts the marks, the lookups, the list, the artists and the queue', async () => {
		await openApp({
			storage: {
				downloaded: { '/a/one': ISO, '/b/two': ISO },
				details: { '/a/one': detailsEntry(), '/b/two': detailsEntry('/b/two') },
				listed: [listedEntry('/a/one'), listedEntry('/b/two'), listedEntry('/c/three')],
				artists: [artistEntry('/a'), artistEntry('/b')]
			},
			session: { queue: [queueEntry('/a/one')] }
		});
		expect(held()).toEqual({ marks: 2, 'looked up': 2, listed: 3, artists: 2, queued: 1 });
	});

	it('counts nothing on a fresh install', async () => {
		await openApp();
		expect(held()).toEqual({ marks: 0, 'looked up': 0, listed: 0, artists: 0, queued: 0 });
	});

	// A lookup made in a SoundCloud tab while the sheet is open, or an import
	// run in another window.
	it('follows what another tab does while it is on screen', async () => {
		await openApp();
		await chrome.storage.local.set({ details: { '/a/one': detailsEntry() } });
		await settle();
		expect(held()['looked up']).toBe(1);
	});
});

// --- exporting ---------------------------------------------------------------

describe('exporting', () => {
	it('offers the whole state as a dated JSON file, whatever the active tab is', async () => {
		await openApp({
			storage: { downloaded: STATE.downloaded, details: STATE.details }
		});
		localStorage.setItem('pref:background', '0');
		localStorage.setItem('pref:sort', 'page');
		localStorage.setItem('pref:hide-downloaded', '1');

		await click(button('export'));
		expect(saved).toEqual([{ name: 'manager-2026-09-08.json', href: 'blob:test/0' }]);
		expect(note()).toBe('saved 1 mark, 1 looked-up track, 3 options to manager-2026-09-08.json.');

		// What went into the blob is the whole state, as the schema describes it.
		const [blob] = createObjectURL.mock.calls[0];
		expect(blob.type).toBe('application/json');
		const written = JSON.parse(await blob.text());
		expect(written).toEqual(STATE);
		expectValid(written);
	});

	it('says so plainly when there is nothing in it yet', async () => {
		await openApp();
		await click(button('export'));
		expect(note()).toBe('saved nothing at all to manager-2026-09-08.json.');
	});

	// The queue is the one part of the file that would otherwise be gone by
	// tomorrow: closing the browser ends it. The two lists are handed over by
	// the stores that own them, as the queue is.
	it('carries the session queue and both lists into the file, and counts them', async () => {
		const bicep = artistEntry('/bicep', { profile: artistProfile() });
		await openApp({
			storage: { listed: [listedEntry('/a/one')], artists: [bicep] },
			session: { queue: [queueEntry('/b/two'), queueEntry('/c/three')] }
		});

		await click(button('export'));
		expect(note()).toBe('saved 1 track listed, 1 artist, 2 queued tracks to manager-2026-09-08.json.');

		const [blob] = createObjectURL.mock.calls[0];
		const written = JSON.parse(await blob.text());
		expect(written.queue).toEqual([queueEntry('/b/two'), queueEntry('/c/three')]);
		expect(written.listed).toEqual([listedEntry('/a/one')]);
		expect(written.artists).toEqual([bicep]);
		expectValid(written);
	});

	// The file is written from the stores, not from storage, and this is the
	// case that tells the two apart: a browser that will not take the write
	// leaves the queue in memory, where the popup is still counting it. A backup
	// that left out exactly what is on screen would be worse than none.
	it('carries the queue the popup is showing, even when none of it could be stored', async () => {
		await openApp({ session: { queue: [queueEntry('/a/one')] } });
		chrome.storage.session.set.mockRejectedValue(new Error('Extension context invalidated'));

		await click(button('export'));
		const [blob] = createObjectURL.mock.calls[0];
		expect(JSON.parse(await blob.text()).queue).toEqual([queueEntry('/a/one')]);
	});

	it.each([
		[new Error('storage is gone'), 'couldn’t export: storage is gone'],
		['storage went away', 'couldn’t export: storage went away']
	])('says so when the state cannot be read (%o)', async (thrown, message) => {
		await openApp();
		chrome.storage.local.get.mockRejectedValue(thrown);
		await click(button('export'));
		expect(note()).toBe(message);
	});
});

// --- what is in the box ------------------------------------------------------

describe('reading what is about to be imported', () => {
	// The point of judging it here: a file chosen by mistake says so before
	// anything is written, and the button that would write it stays off.
	it('says what a good file holds, and when it was written', async () => {
		await openApp();
		await openImport();
		expect(verdict()).toBe(null);
		expect(importNow()!.disabled).toBe(true);

		await type(pasteBox(), JSON.stringify(STATE));
		expect(verdict()).toBe('a backup from 2026-09-08 — 1 mark, 1 looked-up track, 3 options.');
		expect(importNow()!.disabled).toBe(false);
	});

	it('counts the listed and queued tracks in it too', async () => {
		await openApp();
		await openImport();
		await type(pasteBox(), JSON.stringify(file({ listed: [listedEntry('/a/one')], queue: [queueEntry('/b/two')] })));
		expect(verdict()).toBe('a state file — 1 track listed, 1 queued track.');
	});

	it('says a valid file holds nothing when that is what it holds', async () => {
		await openApp();
		await openImport();
		await type(pasteBox(), JSON.stringify(file()));
		expect(verdict()).toBe('a state file — nothing at all.');
		expect(importNow()!.disabled).toBe(false);
	});

	it('names what is wrong with one that cannot be read, and will not import it', async () => {
		await openApp();
		await openImport();

		await type(pasteBox(), 'not json');
		expect(verdict()).toMatch(/^that’s not JSON\./);
		expect(importNow()!.disabled).toBe(true);
		expect(pasteBox()!.getAttribute('aria-invalid')).toBe('true');

		await type(pasteBox(), JSON.stringify({ ...STATE, version: 9 }));
		expect(verdict()).toMatch(/version 9; this reads version 1/);
		expect(importNow()!.disabled).toBe(true);

		// Nothing is written by reading it, whichever way it went.
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});

	it('says nothing at all about an empty box', async () => {
		await openApp();
		await openImport();
		await type(pasteBox(), '   ');
		expect(verdict()).toBe(null);
		expect(importNow()!.disabled).toBe(true);
	});

	// Both routes fill the same box, so what is about to be imported is always
	// the text on screen — and is read by the same eye before it is written.
	it('fills the box from a chosen file, and says so when the file cannot be read', async () => {
		await openApp();
		await openImport();

		const chosen = async (chosenFile: Pick<File, 'text'>) => {
			Object.defineProperty(fileInput()!, 'files', {
				value: [chosenFile],
				configurable: true,
				writable: true
			});
			fileInput()!.dispatchEvent(new Event('change', { bubbles: true }));
			await settle();
		};

		await chosen(new File([JSON.stringify(STATE)], 'state.json', { type: 'application/json' }));
		expect(JSON.parse(pasteBox()!.value)).toEqual(STATE);
		expect(importNow()!.disabled).toBe(false);

		await chosen({ text: () => Promise.reject(new Error('unreadable')) });
		expect(note()).toBe('couldn’t read the file: unreadable');
	});

	it('ignores a file dialog that was dismissed', async () => {
		await openApp();
		await openImport();
		Object.defineProperty(fileInput()!, 'files', {
			value: [],
			configurable: true,
			writable: true
		});
		fileInput()!.dispatchEvent(new Event('change', { bubbles: true }));
		await settle();
		expect(pasteBox()!.value).toBe('');
	});

	// The picker draws whatever the platform draws, which is the one control in
	// this popup that would not look like the rest of it. The button in front of
	// it is what carries the name and the focus.
	it('opens the picker from a button rather than showing the picker', async () => {
		await openApp();
		await openImport();
		const opened = vi.spyOn(fileInput()!, 'click').mockImplementation(() => {});
		await click(button('choose a file'));
		expect(opened).toHaveBeenCalled();
	});

	it('cancels, forgetting what was in the box', async () => {
		await openApp();
		await openImport();
		await type(pasteBox(), JSON.stringify(STATE));

		await click(button('cancel'));
		expect(pasteBox()).toBe(null);
		await click(button('import…'));
		expect(pasteBox()!.value).toBe('');
	});
});

// --- importing, which adds ---------------------------------------------------

describe('importing, which adds to what is here', () => {
	it('restores marks, details and options, and says what it did', async () => {
		await openApp({ storage: { downloaded: { '/b/two': ISO } } });
		await openImport();
		await type(pasteBox(), JSON.stringify(STATE));
		await click(importNow());

		expect(chrome.testStore().downloaded).toEqual({ '/a/one': ISO, '/b/two': ISO });
		expect(chrome.testStore().details).toEqual(STATE.details);
		expect(localStorage.getItem('pref:background')).toBe('0');
		expect(localStorage.getItem('pref:sort')).toBe('page');
		expect(localStorage.getItem('pref:hide-downloaded')).toBe('1');

		expect(note()).toBe(
			'imported 1 new mark, 1 track of download details, 3 options. reload the tab to see the markers.'
		);
		// The drawer is put away, and the file is not left lying in it.
		expect(pasteBox()).toBe(null);
		expect(held()).toEqual({ marks: 2, 'looked up': 1, listed: 0, artists: 0, queued: 0 });
	});

	it('appends the file’s tracks to the list and the queue, holding neither twice', async () => {
		await openApp({
			storage: { listed: [listedEntry('/a/one')] },
			session: { queue: [queueEntry('/a/one')] }
		});
		await openImport();
		await type(
			pasteBox(),
			JSON.stringify(
				file({
					listed: [listedEntry('/a/one', { title: 'already listed' }), listedEntry('/b/two')],
					queue: [queueEntry('/a/one', { title: 'already queued' }), queueEntry('/c/three')]
				})
			)
		);
		await click(importNow());

		expect(note()).toBe(
			'imported 0 new marks, 0 tracks of download details, 1 track onto the list, ' +
				'1 track into the queue. reload the tab to see the markers.'
		);
		expect(chrome.testStore().listed.map((one) => one.permalink)).toEqual(['/a/one', '/b/two']);
		expect(chrome.testStore('session').queue.map((one) => one.permalink)).toEqual(['/a/one', '/c/three']);
		// The entry already queued is the one that stays.
		expect(chrome.testStore('session').queue[0].title).toBe('One');
	});

	it('appends the file’s artists to the artists tool’s list, holding none twice', async () => {
		await openApp({ storage: { artists: [artistEntry('/a')] } });
		await openImport();
		const again = artistEntry('/a', { from: 'elsewhere' });
		await type(pasteBox(), JSON.stringify(file({ artists: [again, artistEntry('/b')] })));
		await click(importNow());

		expect(note()).toBe(
			'imported 0 new marks, 0 tracks of download details, 1 artist onto the artists. ' +
				'reload the tab to see the markers.'
		);
		expect(chrome.testStore().artists.map((one) => one.permalink)).toEqual(['/a', '/b']);
		// The artist already here keeps where it first came from.
		expect(chrome.testStore().artists[0].from).toBe('My Playlist');
		expect(held().artists).toBe(2);
	});

	// Each list stops at what its store holds, and an import past that refuses
	// rather than dropping what is here — and says how many it turned away.
	it('says how many tracks and artists did not fit on a full list', async () => {
		const full = <Entry>(count: number, entry: (i: number) => Entry) =>
			Array.from({ length: count }, (_, i) => entry(i));

		await openApp({
			storage: {
				listed: full(LISTED_MAX, (i) => listedEntry(`/a/t${i}`)),
				artists: full(ARTISTS_MAX, (i) => artistEntry(`/a${i}`))
			}
		});
		await openImport();
		await type(pasteBox(), JSON.stringify(file({ listed: [listedEntry('/b/one')], artists: [artistEntry('/b')] })));
		await click(importNow());

		expect(note()).toBe(
			'imported 0 new marks, 0 tracks of download details. 1 track didn’t fit on the list. ' +
				'1 artist didn’t fit on the artists. reload the tab to see the markers.'
		);
		expect(held()).toMatchObject({ listed: LISTED_MAX, artists: ARTISTS_MAX });
	});

	// The promise, in the one case that would break it: an option somebody has
	// already chosen here is a thing they have, and adding does not overwrite
	// what you have.
	it('leaves an option that is already set, and sets one that is not', async () => {
		localStorage.setItem('pref:sort', 'site');
		await openApp();
		await openImport();
		await type(pasteBox(), JSON.stringify(STATE));
		await click(importNow());

		expect(localStorage.getItem('pref:sort')).toBe('site');
		expect(localStorage.getItem('pref:hide-downloaded')).toBe('1');
		expect(note()).toMatch(/2 options\./);
	});

	it('counts a single restored option in the singular', async () => {
		await openApp();
		await openImport();
		await type(pasteBox(), JSON.stringify(file({ prefs: { sort: 'page' } })));
		await click(importNow());
		expect(note()).toBe(
			'imported 0 new marks, 0 tracks of download details, 1 option. reload the tab to see the markers.'
		);
	});

	it('reports a write that fails, and leaves the box open to try again', async () => {
		await openApp();
		await openImport();
		chrome.storage.local.set.mockRejectedValue(new Error('quota exceeded'));
		await type(pasteBox(), JSON.stringify(STATE));
		await click(importNow());
		expect(note()).toBe('import failed: quota exceeded');
		expect(pasteBox()).not.toBe(null);
		expect(pasteBox()!.value).not.toBe('');
	});
});

// --- importing, asked to replace ---------------------------------------------

describe('importing, asked to replace', () => {
	const armAndImport = async (doc: FieldValue) => {
		await openImport();
		await type(pasteBox(), JSON.stringify(doc));
		await click(replaceSwitch());
		await click(importNow());
	};

	it('is off to begin with, and says what turning it on would do', async () => {
		await openApp();
		await openImport();
		expect(replacing()).toBe(false);
		expect(clean(importNow())).toBe('add to what’s here');
		expect(text()).toContain('replace what’s here instead of adding to it');
		expect(text()).toContain('the file is added to what you have');

		await click(replaceSwitch());
		expect(replacing()).toBe(true);
		expect(clean(importNow())).toBe('replace everything');
	});

	it('makes every part of what is kept exactly what the file says', async () => {
		await openApp({
			storage: {
				downloaded: { '/gone/soon': ISO },
				details: { '/gone/soon': detailsEntry('/gone/soon') },
				listed: [listedEntry('/gone/soon')],
				artists: [artistEntry('/gone')]
			},
			session: { queue: [queueEntry('/gone/soon')] }
		});
		await armAndImport({
			...STATE,
			listed: [listedEntry('/a/one')],
			artists: [artistEntry('/a')],
			queue: [queueEntry('/a/one'), queueEntry('/b/two')]
		});

		expect(chrome.testStore().downloaded).toEqual(STATE.downloaded);
		expect(chrome.testStore().details).toEqual(STATE.details);
		expect(chrome.testStore().listed.map((one) => one.permalink)).toEqual(['/a/one']);
		expect(chrome.testStore().artists.map((one) => one.permalink)).toEqual(['/a']);
		expect(chrome.testStore('session').queue.map((one) => one.permalink)).toEqual(['/a/one', '/b/two']);

		expect(note()).toBe(
			'replaced everything with 1 mark, 1 track of download details, 1 track listed, ' +
				'1 artist, 2 tracks queued, 3 options. reload the tab to see the markers.'
		);
		expect(held()).toEqual({ marks: 1, 'looked up': 1, listed: 1, artists: 1, queued: 2 });
	});

	it('empties what the file is silent about', async () => {
		await openApp({
			storage: {
				downloaded: { '/gone/soon': ISO },
				details: { '/gone/soon': detailsEntry('/gone/soon') },
				listed: [listedEntry('/gone/soon')],
				artists: [artistEntry('/gone')]
			},
			session: { queue: [queueEntry('/gone/soon')] }
		});
		await armAndImport(file());

		expect(note()).toBe(
			'replaced everything with 0 marks, 0 tracks of download details, 0 tracks listed, ' +
				'0 artists, 0 tracks queued. reload the tab to see the markers.'
		);
		expect(held()).toEqual({ marks: 0, 'looked up': 0, listed: 0, artists: 0, queued: 0 });
	});

	it('overrules an option that is already set', async () => {
		localStorage.setItem('pref:sort', 'site');
		await openApp();
		await armAndImport(STATE);
		expect(localStorage.getItem('pref:sort')).toBe('page');
	});

	// The one control here that can lose something, so it is not remembered the
	// way the drawer is: a popup reopened an hour later must not still be armed.
	it('comes back off, and off again after an import', async () => {
		await openApp();
		await openImport();
		await click(replaceSwitch());
		expect(replacing()).toBe(true);

		await mountApp();
		await settle();
		expect(pasteBox()).not.toBe(null);
		expect(replacing()).toBe(false);

		await type(pasteBox(), JSON.stringify(STATE));
		await click(replaceSwitch());
		await click(importNow());
		await click(button('import…'));
		expect(replacing()).toBe(false);
	});
});

// --- where it is, and where it used to be ------------------------------------

describe('getting to it', () => {
	// The list is the biggest thing in the backup, so the Manager's footer is
	// where somebody looks for it; the file carries more than that tool, so the
	// footer points here rather than holding it.
	it('is one press away from the Manager’s footer', async () => {
		panelChrome({ tab: OTHER_TAB });
		await mountApp();
		await click(button('Manager'));
		expect(settingsSheet()).toBe(null);

		await click(button('back up or restore…'));
		expect(settingsSheet()).not.toBe(null);
		expect(section()).toBeTruthy();
	});

	// Choosing a file is itself a way of closing the popup: on some platforms
	// the file dialog takes the focus and the popup goes with it, and coming
	// back to a shut drawer means starting the import over.
	it('comes back open, so the import is not started over', async () => {
		await openApp();
		await openImport();

		await mountApp();
		await settle();
		expect(pasteBox()).not.toBe(null);
	});

	it('comes back shut once it has been cancelled', async () => {
		await openApp();
		await openImport();
		await click(button('cancel'));

		await mountApp();
		await settle();
		expect(pasteBox()).toBe(null);
	});

	// The one text box in the extension that is not kept: what goes into it is a
	// whole state file, which can be megabytes of marks and looked-up details.
	it('does not keep what was pasted into it', async () => {
		await openApp();
		await openImport();
		await type(pasteBox(), JSON.stringify(STATE));

		await mountApp();
		await settle();
		expect(pasteBox()!.value).toBe('');
	});

	// The box grows to its content, which for a state file is tens of thousands
	// of lines; left to do that it would push every control here off the bottom
	// of a popup that is only as tall as what is in it.
	it('caps how tall the paste box can grow', async () => {
		await openApp();
		const box = await openImport();
		expect(box!.className).toMatch(/\bmax-h-\d+\b/);
		expect(box!.className).toMatch(/\boverflow-y-auto\b/);
	});

	it('is a section a screen reader can read', async () => {
		await openApp({ storage: { downloaded: { '/a/one': ISO } } });
		await openImport();
		await type(pasteBox(), JSON.stringify(STATE));
		expect(audit()).toEqual([]);

		await click(replaceSwitch());
		expect(audit()).toEqual([]);
	});
});
