import { describe, expect, it } from 'vitest';
import { activeFilters, type DirectoryFilters, emptyHint } from './directory-filters.ts';

const none: DirectoryFilters = {
	q: '',
	place: '',
	lat: '',
	radiusKm: 50,
	genres: [],
	kinds: [],
	formats: [],
	setTypes: [],
	feeTiers: [],
	country: '',
	unclaimed: false
};

describe('activeFilters', () => {
	it('lists nothing when nothing is on', () => {
		expect(activeFilters(none, new URLSearchParams())).toEqual([]);
	});

	it('names each filter that is on, with the search without it', () => {
		const search = new URLSearchParams(
			'q=neon&place=Berlin&lat=52.5&lng=13.4&radius=25&genres=techno,house&fee=community&setType=closing&format=vinyl&kind=solo&country=DE&unclaimed=on&page=3'
		);

		const filters = {
			...none,
			q: 'neon',
			place: 'Berlin',
			lat: '52.5',
			radiusKm: 25,
			genres: ['techno', 'house'],
			feeTiers: ['community'],
			setTypes: ['closing'],
			formats: ['vinyl'],
			kinds: ['solo'],
			country: 'DE',
			unclaimed: true
		};

		const found = activeFilters(filters, search);

		expect(found.map((filter) => filter.label)).toEqual([
			'“neon”',
			'Within 25 km of Berlin',
			'techno',
			'house',
			'Community',
			'Closing',
			'Vinyl',
			'Solo DJ',
			'DE',
			'Unclaimed only'
		]);

		const without = (label: string) =>
			new URLSearchParams(found.find((filter) => filter.label === label)!.href.split('?')[1]);

		expect(without('techno').getAll('genres')).toEqual(['house']);
		expect(without('techno').has('page')).toBe(false);
		expect(without('Within 25 km of Berlin').has('lat')).toBe(false);
		expect(without('Within 25 km of Berlin').has('place')).toBe(false);
		expect(without('“neon”').get('genres')).toBe('techno,house');
	});

	it('leaves out a place that has no coordinates, and links to the bare page when the last filter goes', () => {
		expect(activeFilters({ ...none, place: 'Atlantis' }, new URLSearchParams('place=Atlantis'))).toEqual([]);
		expect(activeFilters({ ...none, q: 'neon' }, new URLSearchParams('q=neon'))).toEqual([
			{ label: '“neon”', href: '/projects' }
		]);
	});
});

describe('emptyHint', () => {
	it('suggests only what loosens the filters in use', () => {
		expect(emptyHint({ ...none, genres: ['techno'] })).toBe('Try fewer genres.');
		expect(emptyHint({ ...none, place: 'Berlin', lat: '52.5', setTypes: ['closing'], feeTiers: ['community'] })).toBe(
			'Try a wider radius, another slot or another fee tier.'
		);
		expect(emptyHint({ ...none, kinds: ['solo'] })).toBe('Try fewer filters under More filters.');
	});

	it('points a text search at the genres, and says when nobody is listed', () => {
		expect(emptyHint({ ...none, q: 'zzz' })).toBe('Try a shorter search, or a genre from the list.');
		expect(emptyHint(none)).toBe('Nobody is listed yet.');
	});
});
