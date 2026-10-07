import { describe, expect, it } from 'vitest';
import { parseClaim } from './claim';

function form(entries: Record<string, string>): FormData {
	const data = new FormData();

	for (const [key, value] of Object.entries(entries)) data.append(key, value);

	return data;
}

describe('parseClaim', () => {
	it('parses the relationship, evidence, and supporting links', () => {
		expect(
			parseClaim(
				form({
					relationship: 'agent',
					evidence: 'I book them',
					links: 'example.test/bio\nhttps://example.test/bio/  https://example.test/roster'
				})
			)
		).toEqual({
			relationship: 'agent',
			evidence: 'I book them',
			links: ['https://example.test/bio', 'https://example.test/bio/', 'https://example.test/roster']
		});
		expect(parseClaim(form({ relationship: 'self', evidence: 'Me', links: 'https://a.test https://a.test' }))).toEqual({
			relationship: 'self',
			evidence: 'Me',
			links: ['https://a.test/']
		});
		expect(parseClaim(form({ relationship: 'self', evidence: 'Me' }))).toEqual({
			relationship: 'self',
			evidence: 'Me'
		});
	});

	it('refuses unknown relationships, bad links, and more links than a claim holds', () => {
		expect(() => parseClaim(form({ relationship: 'fan', evidence: 'x' }))).toThrow(/how you are connected/);
		expect(() => parseClaim(form({ relationship: 'self', evidence: 'x', links: 'http://[' }))).toThrow(
			/not a valid link/
		);
		const links = Array.from({ length: 11 }, (_, index) => `https://a.test/${index}`).join('\n');
		expect(() => parseClaim(form({ relationship: 'self', evidence: 'x', links }))).toThrow(/up to 10 links/);
	});
});
