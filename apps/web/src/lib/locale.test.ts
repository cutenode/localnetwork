import { describe, expect, it } from 'vitest';
import { languageOf, writerLangs } from './locale.ts';
import { inLocale } from './testing/locale.ts';

describe('languageOf', () => {
	it("is the request's locale, English when nothing asks for another", async () => {
		expect(languageOf()).toBe('en');
		expect(await inLocale('en', () => languageOf())).toBe('en');
	});

	it('reads the en-XA pseudo-locale as English', async () => {
		expect(await inLocale('en-XA', () => languageOf())).toBe('en');
	});
});

describe('writerLangs', () => {
	it("tags what someone writes under en-XA as English, since only the pages' words were dressed up", async () => {
		expect(await inLocale('en-XA', () => writerLangs())).toEqual(['en']);
	});
});
