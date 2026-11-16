import * as fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { type JsonLd, type JsonLdNode, jsonLdScript } from './structured-data';
import { hostileText } from '#lib/testing/hostile-text.ts';

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

describe('jsonLdScript, for any data people write', () => {
	type JsonLdValue = JsonLdNode[string];

	const { node } = fc.letrec<{ value: JsonLdValue; node: JsonLdNode }>((tie) => ({
		value: fc.oneof(
			{ depthSize: 'small' },
			hostileText(),
			fc.integer(),
			fc.boolean(),
			tie('node'),
			fc.array(tie('value'), { maxLength: 3 })
		),
		node: fc
			.tuple(hostileText(), fc.dictionary(hostileText(), tie('value'), { maxKeys: 4 }))
			.map(([type, properties]) => ({ ...properties, '@type': type }))
	}));

	const anyData: fc.Arbitrary<JsonLd> = node.map((root) => ({ ...root, '@context': 'https://schema.org' }));

	it('ends its script element only at the end, opens no comment, and parses back the same', () => {
		fc.assert(
			fc.property(anyData, (data) => {
				const script = jsonLdScript(data);
				const body = script.slice(OPEN.length, -'</script>'.length);

				expect(script.startsWith(OPEN)).toBe(true);
				expect(script.endsWith('</script>')).toBe(true);
				expect(script.match(/<\/script/gi)).toHaveLength(1);
				expect(script.match(/<script/gi)).toHaveLength(1);
				expect(script).not.toContain('<!--');
				expect(body).not.toMatch(/[\u2028\u2029]/);
				expect(JSON.parse(body)).toEqual(data);
			})
		);
	});
});
