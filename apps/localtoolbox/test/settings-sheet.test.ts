// src/lib/components/SettingsSheet.svelte, the header button that opens it,
// and the store underneath both (src/lib/settings.svelte.ts).
//
// Not under test/panels/, because this is not a panel: settings are reached
// from the header of every view rather than from the tool menu, and the shell
// draws them over whichever tool is open. So every test here mounts the whole
// popup, as someone using it would have it.
//
// Two kinds of thing are set here, kept in two places, and which is which is
// the thing most worth holding down. What the SoundCloud page obeys lives in
// chrome.storage.local, because the content script has to be able to read it
// and cannot see the popup's localStorage — a setting written to the wrong
// place is a setting the page never hears about. What the Downloader hands to
// a gate is the popup's own (src/lib/downloader.svelte.ts), read by the panel
// under this sheet rather than by anything outside the popup — and read from
// the *same object*, which is what a value typed here and used there needs to
// be.
import { describe, it, expect } from 'vitest';
import { audit } from './helpers/a11y.ts';
import {
	button,
	buttons,
	click,
	mountApp,
	openQueue,
	openSettings,
	panelChrome,
	queueButton,
	queueSheet,
	settle,
	settleUntil,
	settingsButton,
	settingsSheet,
	text,
	type,
	type Target
} from './helpers/panel.ts';
import type { ChromeMock } from './helpers/chrome.ts';
import { SEARCH_SITES as SITES } from '../src/lib/search-sites.ts';
import manifest from '../src/manifest.json';

declare const chrome: ChromeMock;

// Not SoundCloud at all: settings are not the page, and nothing here should
// depend on what is in front.
const OTHER_TAB = { id: 1, url: 'https://example.com/' };

const openApp = async (options = {}) => {
	panelChrome({ tab: OTHER_TAB, ...options });

	return mountApp();
};

const clean = (node: Target) => node!.textContent.replace(/\s+/g, ' ').trim();

const heading = () => document.querySelector('h1')!.textContent.trim();

const replaceSwitch = () => document.querySelector('#replace-download-buttons');

const isOn = () => replaceSwitch()!.getAttribute('aria-checked') === 'true';

const listViewSwitch = () => document.querySelector('#likes-list-view');

const listViewOn = () => listViewSwitch()!.getAttribute('aria-checked') === 'true';

const stored = () => chrome.testStore().settings;

const field = (id: string) => document.getElementById(id);

const markSwitch = () => field('mark');

const marking = () => markSwitch()!.getAttribute('aria-checked') === 'true';

const themeButton = (label: string) => [...settingsSheet()!.querySelectorAll('button')].find((b) => clean(b) === label);

describe('the button in the header', () => {
	it('is on the tool menu and in every tool', async () => {
		await openApp();
		expect(settingsButton()).toBeTruthy();

		await click(button('playlist to text'));
		expect(heading()).toBe('SoundCloud playlist to text');
		expect(settingsButton()).toBeTruthy();
	});

	it('is the one gesture: it opens the sheet and closes it again', async () => {
		await openApp();
		expect(settingsSheet()).toBe(null);
		expect(settingsButton()!.getAttribute('aria-expanded')).toBe('false');

		await click(settingsButton());
		expect(settingsSheet()).not.toBe(null);
		expect(settingsButton()!.getAttribute('aria-expanded')).toBe('true');

		await click(settingsButton());
		expect(settingsSheet()).toBe(null);
	});

	it('shuts the queue sheet rather than coming up behind it, and the other way round', async () => {
		await openApp();
		await openQueue();
		expect(queueSheet()!.getAttribute('aria-label')).toBe('session queue');

		await click(settingsButton());
		expect(settingsSheet()).not.toBe(null);
		expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(1);

		await click(queueButton());
		expect(settingsSheet()).toBe(null);
		expect(queueSheet()!.getAttribute('aria-label')).toBe('session queue');
	});

	// The popup is destroyed on every click that opens a tab, and a sheet left
	// open is a place like any other (view.svelte.js).
	it('comes back open when the popup is opened again', async () => {
		await openApp();
		await openSettings();
		await settle();

		const session = chrome.testStore('session');
		expect(session.view.settings).toBe(true);

		await mountApp();
		expect(settingsSheet()).not.toBe(null);
	});
});

describe('replacing SoundCloud’s download buttons', () => {
	it('is on to begin with, and says what it does', async () => {
		await openApp();
		await openSettings();
		expect(isOn()).toBe(true);
		expect(text()).toContain('replace SoundCloud’s download buttons');
		// The sentence has to name both of SoundCloud's own controls, because
		// which one is on screen is not this switch's to know.
		expect(text()).toContain('Free Download');
		expect(text()).toContain('Download file');
	});

	it('writes the choice where the content script can read it', async () => {
		await openApp();
		await openSettings();

		await click(replaceSwitch());
		expect(isOn()).toBe(false);
		expect(stored()).toEqual({ replaceDownloadButtons: false, likesListView: false });

		await click(replaceSwitch());
		expect(isOn()).toBe(true);
		expect(stored()).toEqual({ replaceDownloadButtons: true, likesListView: false });
	});

	it('shows what is already stored rather than the default', async () => {
		await openApp({ storage: { settings: { replaceDownloadButtons: false } } });
		await openSettings();
		expect(isOn()).toBe(false);
	});

	it('follows a change made in another window', async () => {
		await openApp();
		await openSettings();
		expect(isOn()).toBe(true);

		await chrome.storage.local.set({ settings: { replaceDownloadButtons: false } });
		await settle();
		expect(isOn()).toBe(false);
	});

	// Stored settings are not this version's to trust: an older install, or an
	// imported file, can hold shapes nothing here expects.
	it.each([
		['nothing stored at all', undefined],
		['a value of the wrong type', { replaceDownloadButtons: 'yes' }],
		['something that is not an object', 'on'],
		['a setting this version has never heard of', { somethingElse: 1 }]
	])('falls back to the default given %s', async (_what, settings) => {
		await openApp({ storage: settings === undefined ? {} : { settings } });
		await openSettings();
		expect(isOn()).toBe(true);
	});
});

// The other switch in that section, and the one setting here that is off to
// begin with: it presses a button on SoundCloud's own page, and SoundCloud
// remembers the press after the visit it was made in.
describe('opening your likes as a list', () => {
	it('is off to begin with, and says what it presses and what it buys', async () => {
		await openApp();
		await openSettings();
		expect(listViewOn()).toBe(false);
		expect(text()).toContain('open your likes as a list');
		// Both halves of SoundCloud's own toggle, by the words on them — the one
		// it presses and the one it leaves you to press back.
		expect(text()).toContain('Badges');
		expect(text()).toContain('List');
		// And why it is worth pressing: a tile is the layout that answers least.
		expect(text()).toContain('genre');
	});

	it('writes the choice beside the other one, where the content script reads both', async () => {
		await openApp();
		await openSettings();

		await click(listViewSwitch());
		expect(listViewOn()).toBe(true);
		expect(stored()).toEqual({ replaceDownloadButtons: true, likesListView: true });

		await click(listViewSwitch());
		expect(listViewOn()).toBe(false);
		expect(stored()).toEqual({ replaceDownloadButtons: true, likesListView: false });
	});

	it('shows what is already stored, and follows a change made in another window', async () => {
		await openApp({ storage: { settings: { likesListView: true } } });
		await openSettings();
		expect(listViewOn()).toBe(true);
		// The other switch is untouched by a stored object that names only one.
		expect(isOn()).toBe(true);

		await chrome.storage.local.set({ settings: { likesListView: false } });
		await settle();
		expect(listViewOn()).toBe(false);
	});

	it.each([
		['nothing stored at all', undefined],
		['a value of the wrong type', { likesListView: 'yes' }],
		['something that is not an object', 'on']
	])('falls back to off given %s', async (_what, settings) => {
		await openApp({ storage: settings === undefined ? {} : { settings } });
		await openSettings();
		expect(listViewOn()).toBe(false);
	});
});

// The address a gate is given, the name beside it, and whether saving a file
// marks its track. They were the Downloader panel's own, in a drawer at the
// bottom of it, which is the wrong place for something typed once: a failed
// row now sends you here instead (see test/panels/third-party-downloads.test.ts).
describe('what the Downloader asks for', () => {
	it('starts empty, marking, and says where the address goes', async () => {
		await openApp();
		await openSettings();
		expect(field('email')).toHaveProperty('value', '');
		expect(field('name')).toHaveProperty('value', '');
		expect(marking()).toBe(true);
		// The hint has to say the address reaches the artist, because a Laylo
		// signup is a mailing list and that is the price of the link.
		expect(text()).toContain('mailing list');
	});

	// The popup's own storage, not the extension's: nothing outside the popup
	// reads these, and they are not in the exported state file either.
	it('keeps them in the popup’s own storage, between openings', async () => {
		await openApp();
		await openSettings();
		await type(field('email'), 'me@example.com');
		await type(field('name'), 'Me');
		await click(markSwitch());

		expect(localStorage.getItem('pref:email')).toBe('me@example.com');
		expect(localStorage.getItem('pref:name')).toBe('Me');
		expect(localStorage.getItem('pref:mark')).toBe('0');
		expect(stored()).toBeUndefined();

		await mountApp();
		expect(field('email')).toHaveProperty('value', 'me@example.com');
		expect(field('name')).toHaveProperty('value', 'Me');
		expect(marking()).toBe(false);
	});

	it('shows what is already stored rather than the defaults', async () => {
		localStorage.setItem('pref:email', 'dj@example.com');
		localStorage.setItem('pref:mark', '0');
		await openApp();
		await openSettings();
		expect(field('email')).toHaveProperty('value', 'dj@example.com');
		expect(marking()).toBe(false);
	});
});

// One switch per site in src/lib/search-sites.json, and nothing in the sheet
// naming a site: the list is data, and a site added to that file has to turn
// up here on its own.
describe('the search badges', () => {
	const badgeSwitch = (id: string) => field(`search-${id}`);
	const badgeOn = (id: string) => badgeSwitch(id)!.getAttribute('aria-checked') === 'true';
	const badgeRows = () => [...settingsSheet()!.querySelectorAll('[id^="search-"]')].map((node) => node.id);

	it('is one switch per site, all on, and says what they are for', async () => {
		await openApp();
		await openSettings();
		expect(badgeRows()).toEqual(SITES.map((site) => `search-${site.id}`));
		expect(SITES.every((site) => badgeOn(site.id))).toBe(true);

		// Each row says the site's name and carries the badge the panel draws.
		for (const site of SITES) expect(text()).toContain(site.name);
		expect(text()).toContain('one badge per site');
	});

	it('keeps a switch in the popup’s own storage, between openings', async () => {
		await openApp();
		await openSettings();

		await click(badgeSwitch('zipdj'));
		expect(badgeOn('zipdj')).toBe(false);
		expect(localStorage.getItem('pref:search:zipdj')).toBe('0');
		// The extension's own storage is for what the SoundCloud page obeys, and
		// nothing outside this popup draws a badge.
		expect(stored()).toBeUndefined();

		// The sheet comes back up with the popup, as it was left.
		await mountApp();
		expect(badgeOn('zipdj')).toBe(false);
		expect(badgeOn('beatport')).toBe(true);

		await click(badgeSwitch('zipdj'));
		expect(localStorage.getItem('pref:search:zipdj')).toBe('1');
	});

	it('shows what is already switched off rather than the defaults', async () => {
		localStorage.setItem('pref:search:bandcamp', '0');
		await openApp();
		await openSettings();
		expect(badgeOn('bandcamp')).toBe(false);
		expect(badgeOn('beatport')).toBe(true);
	});

	// Turning the lot off is allowed, and is a row with nothing on it but its
	// own toggles — which the sheet says out loud rather than leaving somebody
	// to find out on a track they cannot get.
	it('says what all of them off comes to', async () => {
		await openApp();
		await openSettings();
		expect(text()).not.toContain('those tracks get no badges');

		for (const site of SITES) await click(badgeSwitch(site.id));
		expect(SITES.some((site) => badgeOn(site.id))).toBe(false);
		expect(text()).toContain('those tracks get no badges');
	});
});

// The one option here that is about how the popup draws words rather than
// about what anything does. The transform itself is tested in
// test/lib/title-case.test.ts; what matters here is where the switch is kept
// and that the panel under the sheet is holding the same one.
describe('title case', () => {
	const caseSwitch = () => field('title-case');
	const casing = () => caseSwitch()!.getAttribute('aria-checked') === 'true';

	it('is off by default, and says what it does', async () => {
		await openApp();
		await openSettings();
		expect(casing()).toBe(false);
		expect(text()).toContain('title case the title, artist and genre');
		expect(text()).toContain('in an opened row');
	});

	it('keeps it in the popup’s own storage, between openings', async () => {
		await openApp();
		await openSettings();

		await click(caseSwitch());
		expect(casing()).toBe(true);
		expect(localStorage.getItem('pref:title-case')).toBe('1');
		// The extension's own storage is for what the SoundCloud page obeys, and
		// no page draws these fields.
		expect(stored()).toBeUndefined();

		await mountApp();
		expect(casing()).toBe(true);

		await click(caseSwitch());
		expect(localStorage.getItem('pref:title-case')).toBe('0');
	});

	it('shows what is already set rather than the default', async () => {
		localStorage.setItem('pref:title-case', '1');
		await openApp();
		await openSettings();
		expect(casing()).toBe(true);
	});
});

describe('the theme', () => {
	it('is here as well as in the menu header, and sets the same thing', async () => {
		await openApp();
		await openSettings();
		expect(themeButton('system')!.getAttribute('aria-pressed')).toBe('true');

		await click(themeButton('dark'));
		expect(themeButton('dark')!.getAttribute('aria-pressed')).toBe('true');
		expect(!document.documentElement.classList.contains('light')).toBe(true);

		await click(themeButton('light'));
		expect(!document.documentElement.classList.contains('light')).toBe(false);
	});
});

describe('reporting a bug', () => {
	const reportLink = () => [...settingsSheet()!.querySelectorAll('a')].find((link) => clean(link) === 'report a bug');

	const reported = () => Object.fromEntries(new URL(reportLink()!.href).searchParams);

	it('shows the version, and opens GitHub’s form in a tab with it filled in', async () => {
		await openApp();
		await openSettings();

		expect(clean(settingsSheet())).toContain(`localtoolbox ${manifest.version}`);
		expect(reportLink()!.href).toMatch(/^https:\/\/github\.com\/cutenode\/localnetwork\/issues\/new\?/);
		expect(reportLink()!.target).toBe('_blank');
		expect(reportLink()!.rel).toBe('noreferrer');
		// Over the tool menu nothing says which part it is about, so that is left for them to pick.
		expect(reported()).toMatchObject({ template: 'localtoolbox-bug.yml', version: manifest.version });
		expect(reported()).not.toHaveProperty('tool');
		expect(audit()).toEqual([]);
	});

	it('picks the tool under the sheet as the part it is about', async () => {
		await openApp();
		await click(button('playlist to text'));
		await openSettings();

		expect(reported()).toMatchObject({ tool: 'SoundCloud playlist to text', version: manifest.version });
	});
});

describe('getting out', () => {
	it('closes on Escape, on the close button, and on the ground behind it', async () => {
		for (const shut of [
			() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })),
			() =>
				buttons()
					.find((b) => b.getAttribute('aria-label') === 'close settings')!
					.click(),
			() => settingsSheet()!.parentElement!.querySelector<HTMLElement>('button[aria-hidden="true"]')!.click()
		]) {
			await openApp();
			await openSettings();
			expect(settingsSheet()).not.toBe(null);
			shut();
			await settle();
			expect(settingsSheet()).toBe(null);
		}
	});

	it('shuts the view behind it away while it is up', async () => {
		await openApp();
		const main = document.querySelector('main')!;
		expect(main.hasAttribute('inert')).toBe(false);
		await openSettings();
		expect(main.hasAttribute('inert')).toBe(true);
		await click(settingsButton());
		expect(main.hasAttribute('inert')).toBe(false);
	});
});

describe('when the browser says no', () => {
	it('draws the defaults when the settings cannot be read', async () => {
		panelChrome({ tab: OTHER_TAB });
		chrome.storage.local.get.mockRejectedValue(new Error('storage unavailable'));
		await mountApp();
		await openSettings();
		expect(isOn()).toBe(true);
		expect(audit()).toEqual([]);
	});

	it('keeps showing the new value when the write is refused', async () => {
		await openApp();
		await openSettings();
		chrome.storage.local.set.mockRejectedValue(new Error('QUOTA_BYTES quota exceeded'));
		await click(replaceSwitch());
		expect(isOn()).toBe(false);
		expect(audit()).toEqual([]);
	});

	// The sheet is a chunk of its own, for the same reason the panels are, and
	// it can go missing the same way — mid-update, with the popup open.
	it('says so when the sheet’s own chunk cannot be loaded', async () => {
		panelChrome({ tab: OTHER_TAB });
		await mountApp({ chunks: false });
		// mountApp loads the app afresh, so this is the module the popup just imported.
		const { sheetChunk } = await import('../src/lib/settings-sheet.ts');
		sheetChunk.load = () => Promise.reject(new Error('chunk missing'));
		await click(settingsButton());
		await settleUntil(() => /couldn’t open settings/.test(text()));

		expect(text()).toMatch(/couldn’t open settings/);
		expect(document.querySelector('[role="alert"]')).not.toBe(null);
	});
});

describe('the sheet itself', () => {
	it('is a modal dialog a screen reader can read', async () => {
		await openApp();
		const sheet = (await openSettings())!;
		expect(sheet.getAttribute('aria-modal')).toBe('true');
		expect(sheet.getAttribute('aria-label')).toBe('settings');
		expect(audit()).toEqual([]);
	});
});
