// src/lib/search-sites.ts and src/lib/search-sites.svelte.ts — the sites a
// link-less track offers a search on, and which of them are switched on.
//
// The list is data (src/lib/search-sites.json) so that adding a site is one
// object in a file and nothing else, which means these tests are mostly about
// what happens when somebody writes that object wrong: a badge nobody can
// read, an address with a typo in its variable, a line pasted twice. None of
// those may take the panel down with them — a search badge is the smallest
// thing on a row, and the row has a track on it either way.
import { describe, it, expect, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import {
	queryOf,
	readSites,
	searchable,
	searchUrl,
	SEARCH_SITES,
	siteNames,
	VARIABLE_NAMES
} from '../../src/lib/search-sites.ts';

const ROOT = resolve(import.meta.dirname, '../..');

const FILE = JSON.parse(readFileSync(resolve(ROOT, 'src/lib/search-sites.json'), 'utf8'));

const site = (extra = {}) => ({
	id: 'shop',
	name: 'A Shop',
	badge: 'S',
	url: 'https://shop.test/search?q={artist}',
	...extra
});

const one = (extra = {}) => readSites([site(extra)])[0];

const TRACK = { artist: 'Nöm & Co', title: 'Track One' };

describe('the file the sites come from', () => {
	it('describes every site in it well enough to use', () => {
		// Nothing in it is dropped: a line that is quietly ignored is the failure
		// this whole module is built to survive, and the one it must not ship.
		expect(SEARCH_SITES.length).toBe(FILE.length);
		expect(SEARCH_SITES.map((s) => s.id)).toEqual(FILE.map((s: { id: string }) => s.id));
	});

	it('is the four shops the panel has always offered', () => {
		expect(SEARCH_SITES.map((s) => s.name)).toEqual(['Beatport', 'Bandcamp', 'ZIPDJ', 'MyMP3Pool']);
		expect(SEARCH_SITES.map((s) => s.badge)).toEqual(['BP', 'bc', 'Z', 'M']);
		// Switched on to begin with, every one: this is what the badges did
		// before there was anything to switch.
		expect(SEARCH_SITES.every((s) => s.on)).toBe(true);
	});

	it('searches each of them for the artist, encoded', () => {
		const q = encodeURIComponent('Nöm & Co');
		expect(SEARCH_SITES.map((s) => searchUrl(s, TRACK))).toEqual([
			`https://www.beatport.com/search?q=${q}`,
			`https://bandcamp.com/search?q=${q}`,
			`https://www.zipdj.com/app/search?q=${q}`,
			`https://mp3poolonline.com/results/artists?search=${q}`
		]);
	});
});

describe('reading an entry', () => {
	it('keeps what it needs, and takes the file’s order', () => {
		const sites = readSites([site(), site({ id: 'other', name: 'Other', badge: 'O' })]);
		expect(sites.map((s) => s.id)).toEqual(['shop', 'other']);
		expect(sites[0]).toMatchObject({
			id: 'shop',
			name: 'A Shop',
			badge: 'S',
			url: 'https://shop.test/search?q={artist}',
			fields: ['artist'],
			on: true
		});
	});

	it('trims the words, which a hand-edited file is full of', () => {
		expect(one({ id: '  shop  ', name: ' A Shop ', badge: ' S ' })).toMatchObject({
			id: 'shop',
			name: 'A Shop',
			badge: 'S'
		});
	});

	it('ships a site switched off when the file says so', () => {
		expect(one({ on: false }).on).toBe(false);
		expect(one({ on: true }).on).toBe(true);
	});

	// Each of these is a badge that would be there and do nothing, or point
	// somewhere nobody meant. Dropped, and the rest of the file still read.
	it.each([
		['nothing at all', null],
		['a line that is not an object', 'https://shop.test/'],
		['no id', { ...site(), id: '' }],
		['an id of spaces', { ...site(), id: '   ' }],
		['no name', { ...site(), name: undefined }],
		['no badge', { ...site(), badge: '' }],
		['a name that is not text', { ...site(), name: 42 }],
		['no address', { ...site(), url: '' }],
		['an address that is not https', { ...site(), url: 'http://shop.test/?q={artist}' }],
		['an address with no variable in it', { ...site(), url: 'https://shop.test/search' }],
		['a variable that does not exist', { ...site(), url: 'https://shop.test/?q={artis}' }]
	])('drops an entry with %s', (_why, entry) => {
		expect(readSites([entry, site({ id: 'kept' })]).map((s) => s.id)).toEqual(['kept']);
	});

	it('drops a second entry with the same id, which is a line pasted twice', () => {
		const sites = readSites([site(), site({ name: 'Pasted Over' })]);
		expect(sites.map((s) => s.name)).toEqual(['A Shop']);
	});

	// A file somebody has turned into one object rather than a list of them.
	it('reads a file that is not a list as the one entry it is', () => {
		expect(readSites(site()).map((s) => s.id)).toEqual(['shop']);
		expect(readSites({ id: 'broken' })).toEqual([]);
	});
});

describe('a badge’s colours', () => {
	it('are the ones the entry gives', () => {
		expect(one({ background: '#123456', color: '#FFF' }).style).toBe('background:#123456;color:#FFF');
	});

	// Anything else, and the badge is grey with white on it: these land in a
	// style attribute, and a file is not a place to accept CSS from.
	it.each([
		['no colours at all', {}],
		['words instead of colours', { background: 'red', color: 'white' }],
		['a declaration smuggled in', { background: '#fff;position:fixed', color: '#000;z-index:9' }],
		['colours that are not text', { background: 0x123456, color: null }]
	])('fall back to a grey badge for %s', (_why, colours) => {
		expect(one(colours).style).toBe('background:#52525b;color:#fff');
	});
});

describe('filling a template in', () => {
	it('offers the artist, the title, and both', () => {
		expect(VARIABLE_NAMES).toEqual(['artist', 'title', 'query']);
		const url = (template: string) => searchUrl(one({ url: template }), TRACK);
		expect(url('https://shop.test/?a={artist}')).toBe('https://shop.test/?a=N%C3%B6m%20%26%20Co');
		expect(url('https://shop.test/?t={title}')).toBe('https://shop.test/?t=Track%20One');
		expect(url('https://shop.test/?q={query}')).toBe('https://shop.test/?q=N%C3%B6m%20%26%20Co%20Track%20One');
	});

	it('fills every variable in an address, however many times it is named', () => {
		expect(searchUrl(one({ url: 'https://shop.test/{artist}/{title}/{artist}' }), TRACK)).toBe(
			'https://shop.test/N%C3%B6m%20%26%20Co/Track%20One/N%C3%B6m%20%26%20Co'
		);
	});

	it('quotes what the search is for, which is what the button says', () => {
		expect(queryOf(one(), TRACK)).toBe('Nöm & Co');
		expect(queryOf(one({ url: 'https://shop.test/?q={query}' }), TRACK)).toBe('Nöm & Co Track One');
		// The words the track has, in the order the template asks for them.
		expect(queryOf(one({ url: 'https://shop.test/{title}/{artist}' }), TRACK)).toBe('Track One Nöm & Co');
	});

	// A playlist row with no artist on it leaves an {artist} badge nothing to
	// search for, and a search for the empty string is a shop's front page.
	it('says when a track has nothing the template asks for', () => {
		const anon = { artist: '', title: 'Track One' };
		expect(searchable(one(), TRACK)).toBe(true);
		expect(searchable(one(), anon)).toBe(false);
		expect(searchable(one({ url: 'https://shop.test/?t={title}' }), anon)).toBe(true);
		expect(searchable(one({ url: 'https://shop.test/?q={query}' }), anon)).toBe(true);
		expect(searchable(one({ url: 'https://shop.test/?q={query}' }), {})).toBe(false);
		// A track with no title at all, which is a row the page drew without one
		// rather than one whose title is empty; both are the same nothing here.
		expect(searchable(one({ url: 'https://shop.test/?t={title}' }), { artist: 'A' })).toBe(false);
		expect(queryOf(one(), anon)).toBe('');
	});
});

describe('naming them in a sentence', () => {
	it('is the panel’s hint, and reads as one', () => {
		expect(siteNames(SEARCH_SITES)).toBe('Beatport, Bandcamp, ZIPDJ, MyMP3Pool');
		expect(siteNames([])).toBe('');
	});
});

// The switches, which are the popup's own options: one per site, made once so
// that the sheet and the panel under it are throwing the same switch.
describe('which badges are on', () => {
	// A fresh module graph each time: the switches are made as the module is
	// imported, from whatever localStorage held at that moment.
	async function fresh() {
		vi.resetModules();

		return import('../../src/lib/search-sites.svelte.ts');
	}

	it('is every site there is, to begin with', async () => {
		const { searchSites } = await fresh();
		expect(searchSites.all.map((s) => s.on)).toEqual([true, true, true, true]);
		expect(searchSites.enabled.map((s) => s.id)).toEqual(SEARCH_SITES.map((s) => s.id));
	});

	it('remembers a switch under the site’s own id', async () => {
		const { searchSites, searchPrefKey } = await fresh();
		expect(searchPrefKey('zipdj')).toBe('pref:search:zipdj');

		searchSites.set('zipdj', false);
		expect(localStorage.getItem('pref:search:zipdj')).toBe('0');
		expect(searchSites.enabled.map((s) => s.id)).toEqual(['beatport', 'bandcamp', 'mp3pool']);
		expect(searchSites.all.find((s) => s.id === 'zipdj')!.on).toBe(false);

		searchSites.set('zipdj', true);
		expect(localStorage.getItem('pref:search:zipdj')).toBe('1');
		expect(searchSites.enabled.map((s) => s.id)).toContain('zipdj');
	});

	it('starts from what was switched off last time', async () => {
		localStorage.setItem('pref:search:beatport', '0');
		localStorage.setItem('pref:search:mp3pool', '0');
		const { searchSites } = await fresh();
		expect(searchSites.enabled.map((s) => s.id)).toEqual(['bandcamp', 'zipdj']);
	});

	it('can be switched off altogether, which is a row with no badges', async () => {
		const { searchSites } = await fresh();

		for (const listed of SEARCH_SITES) searchSites.set(listed.id, false);
		expect(searchSites.enabled).toEqual([]);
	});
});
