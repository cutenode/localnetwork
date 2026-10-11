// src/lib/filter.ts — matching typed words against a row's text.
import { describe, it, expect } from 'vitest';
import { matches, normalize, HIDE_DOWNLOADED_PREF } from '../../src/lib/filter.ts';

describe('normalize', () => {
	it.each([
		['Bicép', 'bicep'],
		['  Glue  ', 'glue'],
		['Four\tTet\nBaby', 'four tet baby'],
		['ÜNÏ Crème', 'uni creme'],
		['', '']
	])('folds %o to %o', (input, expected) => {
		expect(normalize(input)).toBe(expected);
	});

	it.each([null, undefined])('treats %o as empty', (value) => {
		expect(normalize(value)).toBe('');
	});
});

describe('matches', () => {
	const HAYSTACK = 'Bicep — Glue';

	it.each(['', '   ', 'glue', 'GLUE', 'glue bicep', 'bicep glue'])('finds “%s” in the text', (query) => {
		expect(matches(HAYSTACK, query)).toBe(true);
	});

	it.each(['four tet', 'glue remix', 'bicepglue'])('does not find “%s”', (query) => {
		expect(matches(HAYSTACK, query)).toBe(false);
	});

	it('ignores accents on both sides', () => {
		expect(matches('Ünï — Crème', 'uni creme')).toBe(true);
		expect(matches('Uni — Creme', 'ünï')).toBe(true);
	});
});

it('names the key the exported state file carries the toggle under', () => {
	expect(HIDE_DOWNLOADED_PREF).toBe('pref:hide-downloaded');
});
