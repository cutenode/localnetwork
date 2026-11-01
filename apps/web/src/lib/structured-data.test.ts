import { expect, it } from 'vitest';
import { type JsonLd, jsonLdScript } from './structured-data';

const OPEN = '<script type="application/ld+json">';

it('writes structured data that parses back the same, with nothing in it able to close the tag', () => {
	const data: JsonLd = {
		'@context': 'https://schema.org',
		'@type': 'MusicGroup',
		name: '</script><script>alert(1)</script>',
		description: 'one\u2028two\u2029three <!-- & > done'
	};

	const script = jsonLdScript(data);
	const body = script.slice(OPEN.length, -'</script>'.length);

	expect(script.startsWith(OPEN)).toBe(true);
	expect(body).not.toMatch(/[<\u2028\u2029]/);
	expect(JSON.parse(body)).toEqual(data);
});
