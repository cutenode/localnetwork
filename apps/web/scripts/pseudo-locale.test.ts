import { readFileSync } from 'node:fs';
import * as fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { type Messages, pseudoFile, pseudoLocalize, SOURCE, TARGET } from './pseudo-locale.ts';

describe('pseudoLocalize', () => {
	it('accents every letter and brackets the message', () => {
		expect(pseudoLocalize({ privacy: 'Privacy' })).toEqual({ privacy: '[Ƥřīṽȧȧƈẏ]' });
	});

	it('keeps placeholders, markup and escaped braces as written', () => {
		const { greeting } = pseudoLocalize({ greeting: 'Hi {name}, see {#link}the guide{/link} \\{not a variable\\}' });

		expect(greeting).toMatch(/^\[Ħī \{name\}, şḗḗḗḗ \{#link\}ŧħḗḗ ɠŭŭīḓḗḗ\{\/link\} \\\{.+\\\}\]$/);
	});

	it("pseudo-localizes each variant's pattern and keeps its selectors", () => {
		const shows: Messages = {
			shows: [
				{
					declarations: ['input count', 'local n = count: plural'],
					selectors: ['n'],
					match: { 'n=one': '{count} show', 'n=other': '{count} shows' }
				}
			]
		};

		expect(pseudoLocalize(shows)).toEqual({
			shows: [
				{
					declarations: ['input count', 'local n = count: plural'],
					selectors: ['n'],
					match: { 'n=one': '[{count} şħǿǿẇ]', 'n=other': '[{count} şħǿǿẇş]' }
				}
			]
		});
	});

	it('leaves no Latin letter outside a placeholder, so English that skipped the catalog stands out', () => {
		fc.assert(
			fc.property(fc.string(), fc.stringMatching(/^[a-z]\w*$/), fc.string(), (before, name, after) => {
				const pattern = `${before.replace(/[{}\\]/g, '')}{${name}}${after.replace(/[{}\\]/g, '')}`;

				// SAFETY: a message given as a pattern comes back as a pattern.
				const message = pseudoLocalize({ message: pattern }).message as string;

				expect(message.replace(`{${name}}`, '')).not.toMatch(/[A-Za-z]/);
			})
		);
	});
});

describe('messages/en-XA.json', () => {
	it('is messages/en.json pseudo-localized', () => {
		expect(
			readFileSync(TARGET, 'utf8'),
			'messages/en-XA.json is out of date. Regenerate it with `node scripts/pseudo-locale.ts`'
		).toBe(pseudoFile(readFileSync(SOURCE, 'utf8')));
	});
});
