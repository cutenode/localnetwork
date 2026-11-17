import * as fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { writtenUrl } from '#lib/testing/links.ts';
import { isWebUrl, webHref } from './web-url';

describe('webHref', () => {
	it.each(['https://neon.example/', 'http://neon.example/mix'])('keeps %s', (url) => {
		expect(webHref(url)).toBe(url);
		expect(isWebUrl(url)).toBe(true);
	});

	it.each([
		'javascript:alert(document.domain)',
		'JavaScript:alert(1)',
		'data:text/html,<script>alert(1)</script>',
		'vbscript:msgbox(1)',
		'mailto:dj@neon.example',
		'//neon.example/',
		'neon.example',
		'',
		null,
		undefined
	])('drops %s', (url) => {
		expect(webHref(url)).toBeUndefined();
	});
});

describe('webHref on any address', () => {
	it('gives back only http(s) addresses, as written', () => {
		fc.assert(
			fc.property(fc.option(writtenUrl), (url) => {
				const href = webHref(url);

				if (href === undefined) return;
				expect(href).toBe(url);
				expect(['http:', 'https:']).toContain(new URL(href).protocol);
			})
		);
	});
});
