import { describe, expect, it } from 'vitest';
import { KNOWN_GENRES, normalizeGenre, normalizeGenres } from './genres';

describe('normalizeGenre', () => {
	it('lowercases, trims, collapses whitespace, and applies aliases', () => {
		expect(normalizeGenre('  Deep   House ')).toBe('deep house');
		expect(normalizeGenre('DnB')).toBe('drum and bass');
		expect(normalizeGenre('hip hop')).toBe('hip-hop');
		expect(normalizeGenre('UKG')).toBe('uk garage');
	});

	it('dedupes and caps lists', () => {
		expect(normalizeGenres(['House', 'house', ' HOUSE', 'techno'])).toEqual(['house', 'techno']);
		expect(normalizeGenres(['a', 'b', 'c'], 2)).toEqual(['a', 'b']);
		expect(normalizeGenres(['', 'x'.repeat(49), 'ok'])).toEqual(['ok']);
		// 25 characters but 50 bytes, over the lexicons' 48-byte limit.
		expect(normalizeGenres(['é'.repeat(25), 'é'.repeat(24)])).toEqual(['é'.repeat(24)]);
	});

	it('only applies real aliases, not Object.prototype keys', () => {
		expect(normalizeGenres(['constructor', 'toString'])).toEqual(['constructor', 'tostring']);
	});
});

describe('KNOWN_GENRES', () => {
	it('has unique known genres', () => {
		expect(new Set(KNOWN_GENRES).size).toBe(KNOWN_GENRES.length);
	});
});
