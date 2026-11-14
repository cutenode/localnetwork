// src/lib/search-sites.ts and src/lib/search-sites.svelte.ts: the sites a
// link-less track offers a search on, and which of them are switched on.
//
// The list is data (src/lib/search-sites.json) so that adding a site is one
// object in a file and nothing else, which means these tests are mostly about
// what happens when somebody writes that object wrong: a badge nobody can
// read, an address with a typo in its variable, a line pasted twice. None of
// those may take the panel down with them: a search badge is the smallest
// thing on a row, and the row has a track on it either way.
import { describe, it, expect, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createValidator, explain, SITES_SCHEMA_PATH } from '../../scripts/validate-state.mts';
import {
	entryOf,
	queryOf,
	readSites,
	readTemplate,
	searchable,
	searchUrl,
	SEARCH_SITES,
	siteNames,
	sixDigits,
	VARIABLE_LIST,
	VARIABLE_NAMES
} from '../../src/lib/search-sites.ts';

const ROOT = resolve(import.meta.dirname, '../..');

const read = (path: string) => JSON.parse(readFileSync(resolve(ROOT, path), 'utf8'));

const WHOLE_FILE = read('src/lib/search-sites.json');

const FILE = WHOLE_FILE.sites;

// The file's own schema, which it names in `$schema` for an editor to check it by.
const SCHEMA = read('src/lib/search-sites.schema.json');

const validate = createValidator(SCHEMA);

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
		// Switched on to begin with, every one: an install with no switches
		// set draws the badges it always did.
		expect(SEARCH_SITES.every((s) => s.on)).toBe(true);
	});

	it('names its schema, and holds to it', () => {
		expect(SITES_SCHEMA_PATH).toBe(resolve(ROOT, 'src/lib', WHOLE_FILE.$schema));
		expect(SCHEMA.$schema).toBe('https://json-schema.org/draft/2020-12/schema');
		expect(validate(WHOLE_FILE), explain(validate.errors).join('\n')).toBe(true);
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
	])('drops an entry with %s, which the schema refuses too', (_why, entry) => {
		expect(readSites([entry, site({ id: 'kept' })]).map((s) => s.id)).toEqual(['kept']);
		expect(validate({ sites: [entry] })).toBe(false);
		expect(validate({ sites: [site()] })).toBe(true);
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

// What the settings sheet says under the address field: the same reading a
// shipped entry gets, with the reason it would be dropped.
describe('reading a template', () => {
	it('names the variables in one it can use', () => {
		expect(readTemplate('https://shop.test/{title}/{artist}')).toEqual({ fields: ['title', 'artist'] });
	});

	it.each([
		['http://shop.test/?q={artist}', 'only https addresses work.'],
		['shop.test/?q={artist}', 'only https addresses work.'],
		['https://shop.test/search', 'needs {artist}, {title} or {query} where the search goes.'],
		['https://shop.test/?q={artis}', '{artis} isn’t {artist}, {title} or {query}.'],
		['https://shop.test/{artist}/{track}', '{track} isn’t {artist}, {title} or {query}.']
	])('says why %s cannot be one', (url, problem) => {
		expect(readTemplate(url)).toEqual({ problem });
	});

	it('lists the variables as a sentence would', () => {
		expect(VARIABLE_LIST).toBe('{artist}, {title} or {query}');
	});
});

describe('a site as it is kept', () => {
	it('is what its entry says, colours filled in, and nothing worked out from it', () => {
		expect(entryOf(one({ on: false }))).toEqual({
			id: 'shop',
			name: 'A Shop',
			badge: 'S',
			url: 'https://shop.test/search?q={artist}',
			background: '#52525b',
			color: '#fff'
		});
	});

	// A colour picker takes #rrggbb and nothing else, and shows black for
	// anything it does not; an edit would then save black over the colour.
	it.each([
		['#fff', '#ffffff'],
		['#1e1e1e', '#1e1e1e'],
		['#abcd', '#aabbcc'],
		['#12345678', '#123456']
	])('spells %s out as %s for a colour picker', (hex, six) => {
		expect(sixDigits(hex)).toBe(six);
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

// The sites somebody adds in the settings sheet: kept in the popup's own
// storage in the file's shape, drawn after the file's, each with a switch of
// its own, and never two badges for one search.
describe('sites added in settings', () => {
	async function fresh() {
		vi.resetModules();

		return import('../../src/lib/search-sites.svelte.ts');
	}

	const JUNO = { id: 'juno', name: 'Juno', badge: 'J', url: 'https://www.juno.co.uk/search/?q={query}' };

	const stored = () => JSON.parse(localStorage.getItem('pref:search-sites') ?? 'null');

	it('are none to begin with', async () => {
		const { searchSites } = await fresh();
		expect(searchSites.entries).toEqual([]);
		expect(searchSites.all.every((s) => !s.own)).toBe(true);
	});

	it('go after the file’s, switched on, and are kept between openings', async () => {
		const { searchSites, OWN_SITES_KEY } = await fresh();
		expect(OWN_SITES_KEY).toBe('pref:search-sites');
		expect(searchSites.add([JUNO])).toBe(1);

		expect(searchSites.all.map((s) => [s.id, s.on, s.own])).toEqual([
			['beatport', true, false],
			['bandcamp', true, false],
			['zipdj', true, false],
			['mp3pool', true, false],
			['juno', true, true]
		]);
		expect(searchSites.enabled.at(-1)).toMatchObject({ name: 'Juno', fields: ['query'] });
		// The entry, with the colours it was drawn in, and nothing worked out from it.
		expect(stored()).toEqual([{ ...JUNO, background: '#52525b', color: '#fff' }]);

		const again = await fresh();
		expect(again.searchSites.entries).toEqual([{ ...JUNO, background: '#52525b', color: '#fff' }]);
	});

	it('have a switch of their own, like the file’s', async () => {
		const { searchSites } = await fresh();
		searchSites.add([JUNO]);
		searchSites.set('juno', false);
		expect(localStorage.getItem('pref:search:juno')).toBe('0');
		expect(searchSites.enabled.map((s) => s.id)).not.toContain('juno');

		const again = await fresh();
		expect(again.searchSites.all.find((s) => s.id === 'juno')!.on).toBe(false);
	});

	// Importing the same file twice, or a file with a site already typed in
	// here, must not draw one search twice.
	it('leave out a search that is already a badge, the file’s or an added one', async () => {
		const { searchSites } = await fresh();
		expect(searchSites.add([JUNO, { ...JUNO, id: 'other', name: 'Juno again' }])).toBe(1);
		expect(searchSites.add([JUNO])).toBe(0);
		expect(searchSites.add([{ ...JUNO, id: 'bp', url: SEARCH_SITES[0].url }])).toBe(0);
		expect(searchSites.entries.map((s) => s.name)).toEqual(['Juno']);
	});

	// Two badges sharing a switch would be thrown together.
	it('take a new id where theirs belongs to another search', async () => {
		const { searchSites } = await fresh();
		searchSites.add([{ ...JUNO, id: 'beatport' }]);
		const [added] = searchSites.entries;
		expect(added.name).toBe('Juno');
		expect(added.id).not.toBe('beatport');
		expect(new Set(searchSites.all.map((s) => s.id)).size).toBe(5);
	});

	it('drop what a badge could not use, as the file’s are', async () => {
		const { searchSites } = await fresh();
		expect(searchSites.add([{ ...JUNO, url: 'http://www.juno.co.uk/?q={query}' }, null, 'Juno'])).toBe(0);
		expect(searchSites.entries).toEqual([]);
	});

	it.each([
		['not JSON', '{'],
		['not a list of sites', '{"id":"x"}'],
		['a site no badge could use', '[{"id":"x","name":"X","badge":"X","url":"https://x.test/"}]']
	])('read as none when storage holds %s', async (_why, raw) => {
		localStorage.setItem('pref:search-sites', raw);
		const { searchSites } = await fresh();
		expect(searchSites.entries).toEqual([]);
		expect(searchSites.all).toHaveLength(SEARCH_SITES.length);
	});

	it('are edited in place, keeping their switch', async () => {
		const { searchSites } = await fresh();
		searchSites.add([JUNO, { ...JUNO, id: 'b', name: 'B', url: 'https://b.test/?q={artist}' }]);
		searchSites.set('juno', false);

		searchSites.edit('juno', { name: 'Juno Records', badge: 'JR', url: 'https://juno.test/?q={title}' });
		expect(searchSites.entries.map((s) => [s.id, s.name, s.badge, s.url])).toEqual([
			['juno', 'Juno Records', 'JR', 'https://juno.test/?q={title}'],
			['b', 'B', 'J', 'https://b.test/?q={artist}']
		]);
		expect(searchSites.all.find((s) => s.id === 'juno')!.on).toBe(false);

		// An edit no badge could use changes nothing.
		searchSites.edit('juno', { name: 'Juno', badge: 'J', url: 'https://juno.test/' });
		expect(searchSites.entries[0].name).toBe('Juno Records');
	});

	it('are removed, and only the one asked for', async () => {
		const { searchSites } = await fresh();
		searchSites.add([JUNO, { ...JUNO, id: 'b', name: 'B', url: 'https://b.test/?q={artist}' }]);
		searchSites.remove('juno');
		expect(searchSites.entries.map((s) => s.id)).toEqual(['b']);
		expect(stored().map((s: { id: string }) => s.id)).toEqual(['b']);
	});

	it('become exactly a file’s when an import is asked to replace', async () => {
		const { searchSites } = await fresh();
		searchSites.add([JUNO]);
		expect(searchSites.replace([{ ...JUNO, id: 'b', name: 'B', url: 'https://b.test/?q={artist}' }])).toBe(1);
		expect(searchSites.entries.map((s) => s.id)).toEqual(['b']);
		expect(searchSites.replace([])).toBe(0);
		expect(searchSites.entries).toEqual([]);
	});
});
