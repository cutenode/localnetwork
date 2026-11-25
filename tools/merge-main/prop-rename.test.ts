import { describe, expect, it } from 'vitest';
import { parseScan, propRename } from './prop-rename.ts';

const match = (file: string, text: string, line: number) => JSON.stringify({ file, text, range: { start: { line } } });

describe('parseScan', () => {
	it("reads each match's file, function name and line", () => {
		const stream = `${match('a/src/lib.rs', '#[hegel::test]\nfn r#keeps() {}', 3)}\n${match('a/src/b.rs', '#[hegel::test(test_cases = 5)]\npub fn prop_b() {}', 0)}\n`;

		expect(parseScan(stream)).toEqual([
			{ file: 'a/src/lib.rs', name: 'keeps', line: 4 },
			{ file: 'a/src/b.rs', name: 'prop_b', line: 1 }
		]);
	});

	it('refuses a match without a function', () => {
		expect(() => parseScan(match('a/src/lib.rs', '#[hegel::test]', 6))).toThrow('no fn name in a/src/lib.rs:7');
	});
});

const crates = new Set(['crates/a', 'crates/b']);

const lib = [
	'use crate::helpers::keeps_order;',
	'/// Runs [`keeps_order`] and roundtrip, which `roundtrip` checks.',
	'#[hegel::test]',
	'fn keeps_order() {',
	'    roundtrip(); // keeps_order calls roundtrip',
	'}',
	'#[hegel::test]',
	'fn roundtrip() {}',
	'#[hegel::test]',
	'fn prop_done() {}'
].join('\n');

const tests = [
	{ file: 'crates/a/src/lib.rs', name: 'keeps_order', line: 4 },
	{ file: 'crates/a/src/lib.rs', name: 'roundtrip', line: 8 },
	{ file: 'crates/a/src/lib.rs', name: 'prop_done', line: 10 }
];

describe('propRename', () => {
	it("renames a test's name throughout its crate, and in backticks elsewhere", () => {
		const texts = new Map([
			['crates/a/src/lib.rs', lib],
			['crates/a/README.md', 'keeps_order and roundtrip keep `roundtrip` honest.'],
			['crates/b/README.md', 'See `a::keeps_order()` and [`roundtrip`]; keeps_order is old.'],
			['crates/b/src/lib.rs', 'fn other() {}']
		]);

		const { changed, collisions, notes, leftover } = propRename(tests, texts, crates);

		expect(collisions).toEqual([]);
		expect(notes).toEqual([]);
		expect(Object.fromEntries(changed)).toEqual({
			'crates/a/src/lib.rs': [
				'use crate::helpers::prop_keeps_order;',
				'/// Runs [`prop_keeps_order`] and roundtrip, which `prop_roundtrip` checks.',
				'#[hegel::test]',
				'fn prop_keeps_order() {',
				'    prop_roundtrip(); // prop_keeps_order calls prop_roundtrip',
				'}',
				'#[hegel::test]',
				'fn prop_roundtrip() {}',
				'#[hegel::test]',
				'fn prop_done() {}'
			].join('\n'),
			'crates/a/README.md': 'prop_keeps_order and roundtrip keep `prop_roundtrip` honest.',
			'crates/b/README.md': 'See `a::prop_keeps_order()` and [`prop_roundtrip`]; keeps_order is old.'
		});
		expect(leftover).toEqual([
			'crates/b/README.md:1: See `a::prop_keeps_order()` and [`prop_roundtrip`]; keeps_order is old.'
		]);
	});

	it("renames a name another item in the crate has only in its test's files, and leaves it in backticks elsewhere", () => {
		const texts = new Map([
			['crates/a/src/lib.rs', lib],
			['crates/a/src/other.rs', 'fn roundtrip() {}\nfn x() { roundtrip() }\nstruct S { keeps_order: u8 }'],
			['crates/b/README.md', 'Not `roundtrip`.']
		]);

		const { changed, notes } = propRename(tests, texts, crates);

		expect(notes).toEqual([
			'keeps_order: also crates/a/src/other.rs:3: struct S { keeps_order: u8 }, so renamed only in crates/a/src/lib.rs',
			'roundtrip: also crates/a/src/other.rs:1: fn roundtrip() {}, so renamed only in crates/a/src/lib.rs'
		]);
		expect([...changed.keys()]).toEqual(['crates/a/src/lib.rs']);
	});

	it('ignores items of the same name in other crates and in comments', () => {
		const texts = new Map([
			['crates/a/src/lib.rs', lib],
			['crates/a/src/other.rs', '// let keeps_order = 1;\n// `roundtrip`'],
			['crates/b/src/lib.rs', 'fn roundtrip() {}']
		]);

		const { changed } = propRename(tests, texts, crates);

		expect([...changed.keys()]).toEqual(['crates/a/src/lib.rs', 'crates/a/src/other.rs']);
		expect(changed.get('crates/a/src/other.rs')).toBe('// let prop_keeps_order = 1;\n// `prop_roundtrip`');
	});

	it("refuses, changing nothing, when another item in a test's own file has its name", () => {
		const texts = new Map([['crates/a/src/lib.rs', `${lib}\nconst roundtrip: u8 = 1;`]]);

		const { changed, collisions } = propRename(tests, texts, crates);

		expect(collisions).toEqual(['crates/a/src/lib.rs:11: const roundtrip: u8 = 1;']);
		expect(changed.size).toBe(0);
	});

	it("takes the root's Cargo.toml as the crate of a file outside the others, and no crate without one", () => {
		const texts = new Map([['tests/lib.rs', '#[hegel::test]\nfn keeps_order() {}']]);
		const test = [{ file: 'tests/lib.rs', name: 'keeps_order', line: 2 }];

		expect(propRename(test, texts, new Set(['.'])).changed.get('tests/lib.rs')).toBe(
			'#[hegel::test]\nfn prop_keeps_order() {}'
		);
		expect(propRename(test, texts, new Set()).changed.get('tests/lib.rs')).toBe(
			'#[hegel::test]\nfn prop_keeps_order() {}'
		);
	});
});
