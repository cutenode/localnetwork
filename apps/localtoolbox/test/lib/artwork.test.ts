// src/lib/artwork.ts — full-size artwork URLs.
import { describe, it, expect } from 'vitest';
import { FULL_SIZE, artworkOf, artworkUrlAtSize, fullSizeArtwork } from '../../src/lib/artwork.ts';
import { knownDetails, queueEntry } from '../helpers/state-fixtures.ts';

describe('artworkUrlAtSize', () => {
	it.each([
		['https://i1.sndcdn.com/artworks-x-t120x120.jpg', 'https://i1.sndcdn.com/artworks-x-t500x500.jpg'],
		['https://i1.sndcdn.com/artworks-x-large.jpg', 'https://i1.sndcdn.com/artworks-x-t500x500.jpg'],
		['https://i1.sndcdn.com/avatars-x-CROP.GIF', 'https://i1.sndcdn.com/avatars-x-t500x500.GIF']
	])('rewrites the size token of %s', (url, expected) => {
		expect(artworkUrlAtSize(url, 't500x500')).toBe(expected);
	});

	it.each(['https://example.com/artwork.jpg', 'https://i1.sndcdn.com/artworks-x-t500x500.webp', 'not a url at all'])(
		'passes %s through untouched',
		(url) => {
			expect(artworkUrlAtSize(url, 't500x500')).toBe(url);
		}
	);
});

describe('fullSizeArtwork', () => {
	it('asks for the largest rendition SoundCloud serves', () => {
		expect(FULL_SIZE).toBe('t1080x1080');
		expect(fullSizeArtwork('https://i1.sndcdn.com/artworks-x-t120x120.jpg')).toBe(
			'https://i1.sndcdn.com/artworks-x-t1080x1080.jpg'
		);
		expect(fullSizeArtwork('https://i1.sndcdn.com/artworks-x-original.PNG')).toBe(
			'https://i1.sndcdn.com/artworks-x-t1080x1080.PNG'
		);
		expect(fullSizeArtwork('https://example.com/artwork.jpg')).toBe('https://example.com/artwork.jpg');
	});
});

describe('artworkOf', () => {
	const withArt = 'https://i1.sndcdn.com/artworks-row-t120x120.jpg';
	const fromApi = 'https://i1.sndcdn.com/artworks-api-large.jpg';

	it('shows the thumbnail the row rendered', () => {
		expect(artworkOf(queueEntry({ artworkUrl: withArt, details: knownDetails({ artworkUrl: fromApi }) }))).toBe(
			withArt
		);
	});

	it('falls back to the cover the details name, for a row that never loaded one', () => {
		expect(artworkOf(queueEntry({ artworkUrl: '', details: knownDetails({ artworkUrl: fromApi }) }))).toBe(fromApi);
	});

	it.each([
		['neither', queueEntry({ artworkUrl: '', details: knownDetails({ artworkUrl: '' }) })],
		['details nobody asked for', queueEntry({ artworkUrl: '', details: null })],
		// No cover among them: the details from before covers were looked up.
		['details from an older version', queueEntry({ artworkUrl: '', details: knownDetails() })],
		['nothing at all', undefined]
	])('says nothing for %s', (_name, track) => {
		expect(artworkOf(track)).toBe('');
	});
});
