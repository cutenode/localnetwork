// src/tools/soundcloud-playlist-to-text/Panel.svelte — the text tool.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
	button,
	chooseOption,
	click,
	extraction,
	installClipboard,
	installDownload,
	mountPanel,
	openSelect,
	panelChrome,
	settle,
	text,
	track
} from '../helpers/panel.ts';
import type { ChromeMock } from '../helpers/chrome.ts';
import type { PageType } from '$lib/types';

declare const chrome: ChromeMock;

const TRACKS = [
	track('Bicep', 'Glue'),
	track('Four Tet', 'Baby'),
	track('Twisted', 'Justin Timberlake - Sexyback (EDIT)')
];

const load = () => mountPanel('soundcloud-playlist-to-text');

const output = () => document.querySelector('textarea')!.value;

const meta = () => document.querySelector('header p')!.textContent.trim();

const setOption = async (id: string, value: string) => {
	// The options are localStorage-backed, and the panel reads them as it
	// mounts; setting them before the mount is how a test picks one.
	localStorage.setItem(`pref:${id}`, value);
};

beforeEach(() => {
	// @ts-expect-error -- removes the execCommand a test put there; the DOM types never lack one
	delete document.execCommand;
});

describe('before extraction', () => {
	it('asks for a SoundCloud tab when there is no active tab, or it is elsewhere', async () => {
		for (const tab of [
			null,
			{ id: 1, url: 'https://example.com/' },
			{ id: 2 },
			{ id: 3, url: 'http://soundcloud.com/insecure' }
		]) {
			panelChrome({ tab });
			await load();
			expect(text()).toMatch(/open a SoundCloud playlist, likes or track page/);
			expect(chrome.tabs.sendMessage).not.toHaveBeenCalled();
			document.body.innerHTML = '';
		}
	});

	it('copes with a tab query that fails', async () => {
		panelChrome();
		chrome.tabs.query.mockRejectedValue(new Error('no window'));
		await load();
		expect(text()).toMatch(/open a SoundCloud playlist, likes or track page/);
	});

	it('explains an unreachable page', async () => {
		panelChrome({ response: new Error('Could not establish connection') });
		await load();
		expect(text()).toMatch(/can’t reach the page/);
		expect(chrome.tabs.sendMessage).toHaveBeenCalledWith(7, { type: 'EXTRACT_TRACKS' });
	});

	it.each([undefined, { ok: false }])('reports a failed extraction (%o)', async (response) => {
		panelChrome({ response });
		await load();
		expect(text()).toMatch(/couldn’t read the page\. reload the tab and try again\./);
	});

	it.each<[PageType, number, RegExp]>([
		['playlist', 0, /no tracks here/],
		['likes', 0, /no tracks here\. let the likes page finish loading/],
		['likes', 2, /only liked playlists here \(2 skipped\)/],
		['other', 0, /that’s not a playlist, likes or track page/]
	])('explains an empty list on a %s page, %i playlists skipped', async (pageType, skipped, message) => {
		panelChrome({ response: extraction({ pageType, skippedPlaylists: skipped }) });
		await load();
		expect(text()).toMatch(message);
		expect(document.querySelector('textarea')).toBe(null);
	});

	it('goes back to the menu even when extraction failed', async () => {
		panelChrome({ response: undefined });
		const { onback } = await load();
		await click(document.querySelector('[aria-label="back to all tools"]'));
		expect(onback).toHaveBeenCalled();
	});
});

describe('with tracks', () => {
	beforeEach(() => {
		panelChrome({ response: extraction({ tracks: TRACKS }) });
	});

	it('shows the page summary and the default Artist - Title output', async () => {
		await load();
		expect(meta()).toBe('My Playlist · by Curator · 3 tracks');
		expect(output()).toBe('Bicep - Glue\nFour Tet - Baby\nTwisted - Justin Timberlake - Sexyback (EDIT)');
	});

	it('counts liked playlists the content script skipped', async () => {
		panelChrome({
			response: extraction({ tracks: TRACKS, pageType: 'likes', skippedPlaylists: 1 })
		});
		await load();
		expect(meta()).toMatch(/3 tracks · 1 liked playlist skipped$/);
	});

	it.each([
		['title-artist', 'Glue - Bicep\nBaby - Four Tet\nJustin Timberlake - Sexyback (EDIT) - Twisted'],
		['title-only', 'Glue\nBaby\nJustin Timberlake - Sexyback (EDIT)'],
		['numbered', '1. Bicep - Glue\n2. Four Tet - Baby\n3. Twisted - Justin Timberlake - Sexyback (EDIT)'],
		['tsv', 'Artist\tTitle\nBicep\tGlue\nFour Tet\tBaby\nTwisted\tJustin Timberlake - Sexyback (EDIT)'],
		['csv', 'Artist,Title\nBicep,Glue\nFour Tet,Baby\nTwisted,Justin Timberlake - Sexyback (EDIT)']
	])('renders the %s format', async (format, expected) => {
		await setOption('format', format);
		await load();
		expect(output()).toBe(expected);
	});

	it('renders JSON with only the artist and title', async () => {
		await setOption('format', 'json');
		await load();
		expect(JSON.parse(output())).toEqual([
			{ artist: 'Bicep', title: 'Glue' },
			{ artist: 'Four Tet', title: 'Baby' },
			{ artist: 'Twisted', title: 'Justin Timberlake - Sexyback (EDIT)' }
		]);
	});

	it('quotes CSV fields that carry a comma or a quote', async () => {
		panelChrome({
			response: extraction({ tracks: [track('A, B', 'He said "hi"'), track('Plain', 'Simple')] })
		});
		await setOption('format', 'csv');
		await load();
		expect(output()).toBe('Artist,Title\n"A, B","He said ""hi"""\nPlain,Simple');
	});

	it('takes the artist out of an "Artist - Title" title when asked', async () => {
		await setOption('split-title', '1');
		await load();
		expect(output()).toBe('Bicep - Glue\nFour Tet - Baby\nJustin Timberlake - Sexyback (EDIT)');
	});

	it('removes duplicates case-insensitively, and says how many went', async () => {
		panelChrome({
			response: extraction({
				tracks: [track('Bicep', 'Glue'), track('BICEP', 'GLUE'), track('Four Tet', 'Baby')]
			})
		});
		await setOption('dedupe', '1');
		await load();
		expect(output()).toBe('Bicep - Glue\nFour Tet - Baby');
		expect(text()).toMatch(/2 lines · 1 duplicate removed/);
	});

	it('remembers an option that is changed', async () => {
		await load();
		const dedupe = document.getElementById('dedupe');
		await click(dedupe);
		expect(localStorage.getItem('pref:dedupe')).toBe('1');
	});

	it('falls back to the default format when the stored one is gone', async () => {
		await setOption('format', 'no-such-format');
		await load();
		expect(output()).toMatch(/^Bicep - Glue/);
	});

	it('offers every format by name, and renders the one picked', async () => {
		await load();
		const offered = (await openSelect('output format')).map((option) => option.textContent.trim());
		expect(offered).toEqual(['artist - title', 'title - artist', 'title only', 'numbered list', 'CSV', 'TSV', 'JSON']);

		await chooseOption('output format', 'numbered list');
		expect(output()).toMatch(/^1\. Bicep - Glue/);
		// Picked once is picked for next time, like every other option here.
		expect(localStorage.getItem('pref:format')).toBe('numbered');
		expect(text()).toMatch(/\.txt/);

		await chooseOption('output format', 'JSON');
		expect(JSON.parse(output())).toHaveLength(3);
		expect(text()).toMatch(/\.json/);
	});
});

describe('copying', () => {
	beforeEach(() => {
		panelChrome({ response: extraction({ tracks: TRACKS }) });
	});

	it('copies the output and says so', async () => {
		const clipboard = installClipboard();
		await load();
		await click(button('copy'));
		expect(clipboard.writeText).toHaveBeenCalledWith(output());
		expect(text()).toMatch(/copied/);
	});

	// The button itself says "copied" for a moment, then goes back to offering
	// the copy — so a second one is still one click away.
	it('says “copied” on the button for a moment, then offers it again', async () => {
		vi.useFakeTimers();
		installClipboard();
		await load();
		await click(button('copy'));
		expect(button('copied')).toBeTruthy();

		await vi.advanceTimersByTimeAsync(1500);
		await settle();
		expect(button('copied')).toBeFalsy();
		expect(button('copy')).toBeTruthy();
	});

	it('falls back to a selection when the clipboard API refuses', async () => {
		const clipboard = installClipboard();
		clipboard.writeText.mockRejectedValue(new Error('not focused'));
		const execCommand = vi.fn<typeof document.execCommand>(() => true);
		document.execCommand = execCommand;
		await load();
		await click(button('copy'));
		expect(execCommand).toHaveBeenCalledWith('copy');
		expect(text()).toMatch(/copied/);
	});

	it('says when copying failed altogether', async () => {
		const clipboard = installClipboard();
		clipboard.writeText.mockRejectedValue(new Error('not focused'));
		document.execCommand = vi.fn<typeof document.execCommand>(() => false);
		await load();
		await click(button('copy'));
		expect(text()).toMatch(/couldn’t copy/);
	});
});

describe('downloading', () => {
	it('offers the output as a file named after the playlist', async () => {
		panelChrome({ response: extraction({ tracks: TRACKS, title: 'Sunday Drivers!' }) });
		await load();
		const { link, clicked } = installDownload();
		await click(button('download'));
		expect(link.download).toBe('sunday-drivers.txt');
		expect(clicked).toHaveBeenCalled();
		expect(text()).toMatch(/saved sunday-drivers\.txt/);
	});

	it('uses the format’s extension', async () => {
		panelChrome({ response: extraction({ tracks: TRACKS, title: 'Mix' }) });
		await setOption('format', 'json');
		await load();
		const { link } = installDownload();
		await click(button('download'));
		expect(link.download).toBe('mix.json');
	});

	it('falls back to a generic name when the page has no title', async () => {
		panelChrome({ response: extraction({ tracks: TRACKS, title: '' }) });
		await load();
		const { link } = installDownload();
		await click(button('download'));
		expect(link.download).toBe('soundcloud-tracks.txt');
	});
});

describe('the session queue as the source', () => {
	const QUEUED = [
		{ permalink: '/bicep/glue', title: 'Glue', artist: 'Bicep', artworkUrl: '' },
		{ permalink: '/fourtet/baby', title: 'Baby', artist: 'Four Tet', artworkUrl: '' }
	];

	const sourceButton = (label: string) =>
		[...document.querySelectorAll('button')].find((b) => b.textContent.replace(/\s+/g, ' ').trim().startsWith(label));

	it('turns the queued tracks into text instead of the page’s', async () => {
		panelChrome({ response: extraction({ tracks: TRACKS }), session: { queue: QUEUED } });
		await load();
		expect(output()).toMatch(/^Bicep - Glue\nFour Tet - Baby\nTwisted/);

		await click(sourceButton('session queue'));
		expect(output()).toBe('Bicep - Glue\nFour Tet - Baby');
		expect(meta()).toBe('session queue · 2 tracks');
		expect(localStorage.getItem('pref:source')).toBe('queue');
	});

	it('works from the queue with no page to read at all', async () => {
		await setOption('source', 'queue');
		panelChrome({ tab: { id: 1, url: 'https://example.com/' }, session: { queue: QUEUED } });
		await load();
		expect(output()).toBe('Bicep - Glue\nFour Tet - Baby');
		expect(text()).not.toMatch(/open a SoundCloud playlist, likes or track page/);
	});

	it('names the saved file after the queue', async () => {
		await setOption('source', 'queue');
		const { link } = installDownload();
		panelChrome({ session: { queue: QUEUED } });
		await load();
		await click(button('download'));
		expect(link.download).toBe('session-queue.txt');
	});

	// A queued track may carry no title or artist — a state file can hold just
	// the permalink — and reads as a row that named none: blank, not "undefined",
	// and not a panel that falls over when the artist is pulled out of the title.
	it('turns a queued track with no title or artist into a blank line', async () => {
		await setOption('source', 'queue');
		await setOption('split-title', '1');
		await setOption('format', 'tsv');
		panelChrome({
			session: {
				queue: [
					{ permalink: '/someone/something' },
					{ permalink: '/channel/upload', title: 'Bicep - Glue' },
					{ permalink: '/fourtet/baby', artist: 'Four Tet' }
				]
			}
		});
		await load();
		expect(output()).toBe('Artist\tTitle\n\t\nBicep\tGlue\nFour Tet\t');
		expect(text()).toMatch(/3 lines/);

		await chooseOption('output format', 'title only');
		expect(output()).toBe('\nGlue\n');
		await chooseOption('output format', 'JSON');
		expect(JSON.parse(output())).toEqual([
			{ artist: '', title: '' },
			{ artist: 'Bicep', title: 'Glue' },
			{ artist: 'Four Tet', title: '' }
		]);
	});

	it('says where an empty queue is filled from', async () => {
		await setOption('source', 'queue');
		panelChrome();
		await load();
		expect(document.querySelector('textarea')).toBe(null);
		expect(text()).toMatch(/nothing queued yet/);
	});
});

describe('when localStorage is unavailable', () => {
	it('still renders, with the default options', async () => {
		const real = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
		Object.defineProperty(globalThis, 'localStorage', {
			configurable: true,
			get() {
				throw new Error('denied');
			}
		});

		try {
			panelChrome({ response: extraction({ tracks: TRACKS }) });
			await load();
			await settle();
			expect(output()).toMatch(/^Bicep - Glue/);
		} finally {
			Object.defineProperty(globalThis, 'localStorage', real!);
		}
	});
});
