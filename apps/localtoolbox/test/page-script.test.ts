// @vitest-environment node
//
// scripts/page-script.mts: what the build ships for each page script. The
// comments go, and everything else stays on the line and column it had in the
// source, which is what lets an error on soundcloud.com point at content.ts
// without a sourcemap.
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { parseSync } from 'oxc-parser';
import blankSpace from 'ts-blank-space';
import { pageScript } from '../scripts/page-script.mts';

const ROOT = resolve(import.meta.dirname, '..');

const PAGE_SCRIPTS = ['content', 'gate-page', 'downloaded-store', 'state-transfer', 'theme-prepaint'];

// Every syntax node the parser finds, with its start and end as line:column
// rather than offsets, one field to a line so a mismatch reads as a diff.
function layout(code: string) {
	const where: string[] = [];

	code.split('\n').forEach((line, n) => {
		for (let column = 0; column <= line.length; column++) where.push(`${n + 1}:${column}`);
	});

	const { program, errors } = parseSync('page-script.js', code);
	expect(errors).toEqual([]);

	return JSON.stringify(program, (key, field) => (key === 'start' || key === 'end' ? where[field] : field), 1).split(
		'\n'
	);
}

describe('a page script as it ships', () => {
	it.each(PAGE_SCRIPTS)('%s.js has no comments, and every node where the source has it', (name) => {
		const source = readFileSync(resolve(ROOT, `${name}.ts`), 'utf8');
		const shipped = pageScript(source);

		expect(parseSync('page-script.js', shipped).comments).toEqual([]);
		expect(layout(shipped)).toEqual(layout(blankSpace(source)));
		// The point of it: a page script is mostly comment.
		expect(shipped.length).toBeLessThan(blankSpace(source).length);
	});

	it('takes only comments, wherever else `//` and `/*` turn up', () => {
		const source = [
			"const url = 'https://soundcloud.com/x'; // the page",
			'const re = /\\/\\/[^/]*\\/*/g;',
			'const ratio = a / b / c; /* inline */ const d = 1;',
			'const t = `// not a comment ${/* is one */ 1} /* nor this */`;',
			'const multi = `line   ',
			'  // still the template',
			'`;',
			'\t// indented',
			'/**',
			' * A docblock.',
			' */',
			'/* one',
			' two */ const e = 2;',
			'function f(x: number /* typed */): number {',
			'\treturn x; /* a */ // b',
			'}',
			'/*! a licence */',
			'/** @license MIT */',
			'// the end'
		].join('\n');

		expect(pageScript(source).split('\n')).toEqual([
			"const url = 'https://soundcloud.com/x';",
			'const re = /\\/\\/[^/]*\\/*/g;',
			'const ratio = a / b / c;              const d = 1;',
			'const t = `// not a comment ${             1} /* nor this */`;',
			'const multi = `line   ',
			'  // still the template',
			'`;',
			'',
			'',
			'',
			'',
			'',
			'        const e = 2;',
			'function f(x                    )         {',
			'\treturn x;',
			'}',
			'/*! a licence */',
			'/** @license MIT */',
			''
		]);
	});
});
