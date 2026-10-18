import { describe, expect, it } from 'vitest';
import { artistHref, bareMbid, chartFilters, monthLabel, recentMonths, trackArtists } from './supports.ts';

const TODAY = new Date('2026-10-16T09:30:00Z');

const filters = (search: string) => chartFilters(new URLSearchParams(search), TODAY);

describe('chartFilters', () => {
	it('looks at the last 30 days up to today everywhere by default, leaving out sets dated later', () => {
		expect(filters('')).toEqual({ month: null, place: null, query: { since: '2026-09-17', before: '2026-10-17' } });
	});

	it('takes a month as its days, and a place as its city and country', () => {
		expect(filters('month=2026-12&place=berlin, de')).toEqual({
			month: '2026-12',
			place: 'berlin, DE',
			query: { since: '2026-12-01', before: '2027-01-01', city: 'berlin', country: 'DE' }
		});
	});

	it('ignores a month or place it can’t read', () => {
		expect(filters('month=2026-13&place=Berlin')).toEqual(filters(''));
		expect(filters('month=last&place=, DE')).toEqual(filters(''));
	});
});

describe('recentMonths', () => {
	it('lists this month first, back across the year', () => {
		expect(recentMonths(TODAY, 12)).toEqual([
			'2026-10',
			'2026-09',
			'2026-08',
			'2026-07',
			'2026-06',
			'2026-05',
			'2026-04',
			'2026-03',
			'2026-02',
			'2026-01',
			'2025-12',
			'2025-11'
		]);
		expect(monthLabel('2026-01')).toBe('January 2026');
	});
});

describe('artists', () => {
	it('reads MusicBrainz IDs in either form as bare lowercase UUIDs', () => {
		expect(bareMbid('mbid:2A8E4F60-1C3B-4D7E-9A05-6B1F0C2D3E41')).toBe('2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41');
		expect(bareMbid('2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41')).toBe('2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41');
		expect(bareMbid('Ferrule')).toBeUndefined();
	});

	it('credits a track’s artists one by one, or its whole credit', () => {
		const ferrule = { artistName: 'Ferrule', artistMbId: '2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41' };

		expect(trackArtists({ artist: 'Ferrule & Ava Lind', artists: [ferrule, { artistName: 'Ava Lind' }] })).toEqual([
			{ name: 'Ferrule', mbid: ferrule.artistMbId },
			{ name: 'Ava Lind', mbid: undefined }
		]);
		expect(trackArtists({ artist: 'Ferrule, Ava Lind' })).toEqual([{ name: 'Ferrule, Ava Lind' }]);
		expect(trackArtists({})).toEqual([]);
	});

	it('links to an artist by MBID when they have one, else by a name no URL can mistake for anything else', () => {
		const mbid = '2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41';

		expect(artistHref({ name: 'Ferrule', mbid })).toBe(`/artists/${mbid}`);
		expect(artistHref({ name: 'AC/DC & Friends' })).toBe('/artists/name/AC%2FDC%20%26%20Friends');
		expect(artistHref({ name: mbid })).toBe(`/artists/name/${mbid}`);
		expect(artistHref({ name: '..' })).toBe('/artists/name?name=..');
		expect(artistHref({ name: '...' })).toBe('/artists/name/...');
	});
});
