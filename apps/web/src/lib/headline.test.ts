import { describe, expect, it } from 'vitest';
import headline from './headline.json' with { type: 'json' };

// headline.schema.json checks this in an editor; this keeps a bad edit from reaching the homepage.
describe('headline.json', () => {
	it.each(['verbs', 'nouns'] as const)('has %s, each one written once with no stray spaces', (list) => {
		const words = headline[list];

		expect(words.length).toBeGreaterThan(0);
		expect(new Set(words).size).toBe(words.length);
		expect(words.filter((word) => word.trim() !== word || word === '')).toEqual([]);
	});
});
