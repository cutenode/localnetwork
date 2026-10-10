import { describe, expect, it } from 'vitest';
import { diff, flatten } from './diff';

describe('flatten', () => {
	it('flattens nested values into dotted paths', () => {
		expect(
			flatten({
				$type: 'x',
				name: 'A',
				genres: ['house', 'disco'],
				location: { city: 'Berlin', latE6: 1 },
				links: [{ kind: 'website', url: 'https://a' }],
				avatar: { $type: 'blob', ref: { $link: 'bafyabcdefgh12345678' }, mimeType: 'image/png', size: 1 },
				empty: '',
				none: null,
				list: []
			})
		).toEqual({
			name: 'A',
			genres: 'house, disco',
			'location.city': 'Berlin',
			'location.latE6': '1',
			'links[0].kind': 'website',
			'links[0].url': 'https://a',
			avatar: 'image 12345678'
		});
	});

	it('labels non-image blobs as tracklists', () => {
		expect(
			flatten({
				mixes: [
					{
						title: 'Set',
						tracklist: { $type: 'blob', ref: { $link: 'bafytracklist87654321' }, mimeType: 'application/json', size: 2 }
					}
				]
			})
		).toEqual({ 'mixes[0].title': 'Set', 'mixes[0].tracklist': 'tracklist 87654321' });
	});
});

describe('diff', () => {
	it('reports added, removed, and changed fields with a prefix', () => {
		expect(diff({ a: 1, b: 'x' }, { a: 2, c: true }, 'p')).toEqual([
			{ field: 'p.a', before: '1', after: '2' },
			{ field: 'p.b', before: 'x' },
			{ field: 'p.c', after: 'true' }
		]);
		expect(diff({ a: 1 }, { a: 1 })).toEqual([]);
	});
});
