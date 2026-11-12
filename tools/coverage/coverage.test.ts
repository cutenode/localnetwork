import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { type Floors, type Percents, type Report, byCrate, crateOf, problems, ratcheted, run } from './coverage.ts';

const counts = (count: number, covered: number) => ({
	lines: { count, covered },
	functions: { count, covered },
	regions: { count, covered }
});

const report = (...files: [string, number, number][]): Report => ({
	data: [{ files: files.map(([filename, count, covered]) => ({ filename, summary: counts(count, covered) })) }]
});

const all = (value: number): Percents => ({ lines: value, functions: value, regions: value });

describe('crateOf', () => {
	it.each([
		['/home/runner/work/localnetwork/localnetwork/crates/loclib-model/src/lib.rs', 'crates/loclib-model'],
		['apps/locallibrary/src/tui/app.rs', 'apps/locallibrary'],
		['plugins/sound/src/lib.rs', 'plugins/sound'],
		['vendor/rekordbox-pdb/src/lib.rs', 'vendor/rekordbox-pdb']
	])('puts %s in %s', (filename, crate) => {
		expect(crateOf(filename)).toBe(crate);
	});

	it('leaves out a file in no crate', () => {
		expect(crateOf('/rustc/library/core/src/option.rs')).toBeUndefined();
	});
});

describe('byCrate', () => {
	it("adds up each crate's files, and leaves out files in none", () => {
		const measured = byCrate(
			report(
				['/w/crates/a/src/lib.rs', 10, 9],
				['/w/crates/a/src/b.rs', 10, 10],
				['/w/plugins/p/src/lib.rs', 4, 1],
				['/rustc/core.rs', 100, 0]
			)
		);

		expect([...measured]).toEqual([
			['crates/a', all(95)],
			['plugins/p', all(25)]
		]);
	});

	it('counts a crate with nothing to cover as covered', () => {
		expect(byCrate(report(['crates/empty/src/lib.rs', 0, 0])).get('crates/empty')).toEqual(all(100));
	});
});

describe('problems', () => {
	const measured = new Map([['crates/a', { lines: 90, functions: 95, regions: 80 }]]);

	it('passes a crate at or over its floor', () => {
		expect(problems(measured, { 'crates/a': { lines: 90, functions: 95, regions: 80 } })).toEqual([]);
	});

	it('names each metric under its floor', () => {
		expect(problems(measured, { 'crates/a': { lines: 91, functions: 95, regions: 80.5 } })).toEqual([
			'crates/a: lines 90.00% is under its floor of 91%',
			'crates/a: regions 80.00% is under its floor of 80.5%'
		]);
	});

	it('fails a crate with no floor, and a floor with no crate', () => {
		expect(problems(measured, { 'crates/gone': all(50) })).toEqual([
			'crates/a: no floor; run the ratchet and commit tools/coverage/floors.json',
			'crates/gone: has a floor but no coverage; remove it from tools/coverage/floors.json'
		]);
	});
});

describe('ratcheted', () => {
	it('raises each floor to just under the measurement, never lowers one, and adds new crates in order', () => {
		const measured = new Map([
			['crates/z', { lines: 96.81, functions: 100, regions: 0.1 }],
			['crates/a', all(90)]
		]);

		expect(ratcheted(measured, { 'crates/a': all(95), 'crates/old': all(10) })).toEqual({
			'crates/a': all(95),
			'crates/z': { lines: 96.6, functions: 99.8, regions: 0 }
		});
	});
});

describe('the commands', () => {
	const start = process.cwd();
	let dir: string;

	const floors = (values: Floors['floors']) =>
		writeFileSync(
			'tools/coverage/floors.json',
			`${JSON.stringify({ $schema: './floors.schema.json', floors: values }, null, '\t')}\n`
		);

	beforeEach(() => {
		dir = mkdtempSync(join(tmpdir(), 'coverage-'));
		process.chdir(dir);
		mkdirSync('tools/coverage', { recursive: true });
		writeFileSync('coverage.json', JSON.stringify(report(['crates/a/src/lib.rs', 10, 9])));
	});
	afterEach(() => {
		process.chdir(start);
		rmSync(dir, { recursive: true });
	});

	it('check passes a run over every floor', () => {
		floors({ 'crates/a': all(90) });
		expect(run(['check', 'coverage.json'])).toEqual({ lines: [], ok: true });
	});

	it('check fails a run under a floor', () => {
		floors({ 'crates/a': all(95) });
		expect(run(['check', 'coverage.json']).ok).toBe(false);
	});

	it('ratchet writes the raised floors', () => {
		floors({ 'crates/a': all(50) });
		expect(run(['ratchet', 'coverage.json'])).toEqual({ lines: ['wrote tools/coverage/floors.json'], ok: true });
		expect(JSON.parse(readFileSync('tools/coverage/floors.json', 'utf8'))).toEqual({
			$schema: './floors.schema.json',
			floors: { 'crates/a': all(89.8) }
		});
	});

	it.each([[[]], [['check']], [['other', 'coverage.json']], [['check', 'coverage.json', 'extra']]])(
		'%j prints how to use it',
		(args) => {
			expect(run(args)).toEqual({ lines: ['usage: cli.ts check|ratchet <coverage.json>'], ok: false });
		}
	);
});
