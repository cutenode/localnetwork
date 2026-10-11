// src/tools/soundcloud-playlist-to-text/formats.ts — turning tracks into text.
import { describe, it, expect } from 'vitest';
import {
	FORMATS,
	FORMAT_VALUES,
	dedupeTracks,
	formatFor,
	processTracks,
	slugify,
	splitTitleArtist
} from '../../src/tools/soundcloud-playlist-to-text/formats.ts';

const t = (artist: string, title: string) => ({ artist, title });

describe('splitTitleArtist', () => {
	it.each([
		['Artist - Song', { artist: 'Artist', title: 'Song' }],
		['Artist – Song', { artist: 'Artist', title: 'Song' }],
		['Artist — Song', { artist: 'Artist', title: 'Song' }],
		// The earliest separator wins, whichever kind it is.
		['Artist – Song - Official Video', { artist: 'Artist', title: 'Song - Official Video' }],
		['Artist - Song – Remix', { artist: 'Artist', title: 'Song – Remix' }]
	])('splits %o at the artist boundary', (title, expected) => {
		expect(splitTitleArtist(t('Channel', title))).toEqual(expected);
	});

	it.each([
		'No separator here',
		// Nothing before the dash, or nothing after it: not an artist boundary.
		' - Song',
		'Artist - '
	])('leaves %o alone', (title) => {
		expect(splitTitleArtist(t('Channel', title))).toEqual(t('Channel', title));
	});
});

describe('dedupeTracks', () => {
	it('drops repeats regardless of case, keeping the first', () => {
		expect(dedupeTracks([t('Bicep', 'Glue'), t('BICEP', 'GLUE'), t('Four Tet', 'Baby')])).toEqual([
			t('Bicep', 'Glue'),
			t('Four Tet', 'Baby')
		]);
	});

	it('does not confuse two tracks whose artist/title boundary differs', () => {
		const tracks = [t('Bicep', 'Glue Remix'), t('Bicep Glue', 'Remix')];
		expect(dedupeTracks(tracks)).toEqual(tracks);
	});
});

describe('processTracks', () => {
	const TRACKS = [t('Channel', 'Bicep - Glue'), t('Bicep', 'Glue')];

	it('does nothing by default', () => {
		expect(processTracks(TRACKS)).toEqual(TRACKS);
	});

	it('splits and dedupes together, in that order', () => {
		expect(processTracks(TRACKS, { splitTitle: true, dedupe: true })).toEqual([t('Bicep', 'Glue')]);
	});

	// A queue entry need not name either: a state file may hold just the
	// permalink, and a page's button off a row nobody recognised queues one
	// with neither. Absent reads as '' does, which is what a row that named
	// none gives — in every format, and under both options.
	it('reads a missing artist or title as empty', () => {
		const queued = [
			{ permalink: '/a/one' },
			{ permalink: '/a/two', title: 'Bicep - Glue' },
			{ permalink: '/a/three', artist: 'Four Tet' }
		];

		const blank = [t('', ''), t('', 'Bicep - Glue'), t('Four Tet', '')];
		expect(processTracks(queued)).toEqual(blank);
		expect(processTracks(queued, { splitTitle: true, dedupe: true })).toEqual([
			t('', ''),
			t('Bicep', 'Glue'),
			t('Four Tet', '')
		]);

		for (const format of FORMATS) {
			expect(format.render(processTracks(queued)), format.value).toBe(format.render(blank));
		}
	});
});

describe('the formats', () => {
	it('offers every format the panel lists, and falls back to the first', () => {
		const offered = ['artist-title', 'title-artist', 'title-only', 'numbered', 'csv', 'tsv', 'json'];
		expect(FORMATS.map((f) => f.value)).toEqual(offered);
		expect(FORMAT_VALUES).toEqual(offered);
		expect(formatFor('csv')).toMatchObject({ value: 'csv', ext: 'csv' });
		expect(formatFor('no-such-format')).toBe(FORMATS[0]);
	});

	it('gives each one an extension and a media type', () => {
		for (const format of FORMATS) {
			expect(format.ext, format.value).toMatch(/^[a-z]+$/);
			expect(format.mime, format.value).toMatch(/^[a-z]+\/[a-z-]+$/);
			expect(format.label, format.value).toBeTruthy();
		}
	});
});

describe('slugify', () => {
	it.each([
		['Sunday Drivers!', 'sunday-drivers'],
		['  Café  Nights  ', 'cafe-nights'],
		['///', 'soundcloud-tracks'],
		['', 'soundcloud-tracks']
	])('turns %o into %o', (title, expected) => {
		expect(slugify(title)).toBe(expected);
	});

	it.each([null, undefined])('falls back for %o', (value) => {
		expect(slugify(value)).toBe('soundcloud-tracks');
	});
});
