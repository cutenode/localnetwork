import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { Report } from '../coverage/coverage.ts';
import { type Numbers, drops, measure, run, table } from './report.ts';

const ROOT = '/home/runner/work/localnetwork/localnetwork';

const file = (filename: string, count: number, covered: number) => ({
	filename,
	summary: { lines: { count, covered }, functions: { count: 2, covered: 1 }, regions: { count: count * 2, covered } }
});

const exported = (...files: ReturnType<typeof file>[]): Report => ({ data: [{ files }] });

describe('measure', () => {
	it("counts only the files the target's paths name", () => {
		const report = exported(
			file(`${ROOT}/crates/loclib-anlz/src/lib.rs`, 200, 150),
			file(`${ROOT}/crates/loclib-anlz/src/grid.rs`, 100, 50),
			// Left out: what every target leaves out, what the paths don't name, and the crates it's built on.
			file(`${ROOT}/crates/loclib-anlz/src/tests.rs`, 100, 0),
			file(`${ROOT}/crates/loclib-model/src/lib.rs`, 100, 0),
			file('/home/runner/.cargo/registry/src/index.crates.io-0/byteorder-1.5.0/src/lib.rs', 100, 0)
		);

		expect(measure(report, ['crates/loclib-anlz/src/**'], ROOT)).toEqual({
			lines: 66.67,
			functions: 50,
			regions: 33.33
		});
	});

	it('gives 0% when none of its files were built', () => {
		expect(measure(exported(), ['crates/loclib-anlz/src/**'], ROOT)).toEqual({ lines: 0, functions: 0, regions: 0 });
	});
});

const all = (value: number) => ({ lines: value, functions: value, regions: value });

describe('drops', () => {
	it('names each metric that fell by a tenth of a point or more', () => {
		const now: Numbers = { anlz: { lines: 80, functions: 70, regions: 59.95 }, wav: all(50) };

		expect(drops(now, { anlz: all(60), wav: all(50.1) })).toEqual([
			'wav: lines 50% (was 50.1%), functions 50% (was 50.1%), regions 50% (was 50.1%)'
		]);
		expect(drops(now, { anlz: { lines: 80, functions: 72.5, regions: 60 } })).toEqual([
			'anlz: functions 70% (was 72.5%)'
		]);
	});

	it('counts a tenth that floating point makes a little less, and no smaller fall', () => {
		expect(drops({ wav: all(60.2) }, { wav: all(60.3) })).toHaveLength(1);
		expect(drops({ wav: all(60.21) }, { wav: all(60.3) })).toEqual([]);
	});

	it('leaves alone a target with no number last week', () => {
		expect(drops({ peers: all(10) }, { anlz: all(90) })).toEqual([]);
	});
});

describe('table', () => {
	const now: Numbers = { anlz: { lines: 80.5, functions: 70, regions: 60.25 }, peers: all(10) };

	it("gives each target's change since main's last weekly run", () => {
		expect(table(now, { anlz: { lines: 80, functions: 71.25, regions: 60.25 } })).toEqual([
			'## Fuzz coverage',
			'',
			"Each target's corpus replayed, as a share of the code its header names (`// coverage:`, or else `// paths:`), in points since main's last weekly run.",
			'',
			'| Target | Lines | Functions | Regions |',
			'| --- | ---: | ---: | ---: |',
			'| anlz | 80.5% (+0.5) | 70% (-1.25) | 60.25% (same) |',
			'| peers | 10% | 10% | 10% |'
		]);
	});

	it('says when there was nothing to compare with', () => {
		expect(table(now, undefined).slice(2, 3)).toEqual([
			"Each target's corpus replayed, as a share of the code its header names (`// coverage:`, or else `// paths:`). No weekly numbers from main to compare with."
		]);
	});
});

describe('run', () => {
	let root: string;

	beforeEach(() => {
		root = mkdtempSync(join(tmpdir(), 'fuzz-report-'));
		mkdirSync(join(root, 'fuzz/fuzz_targets'), { recursive: true });
		mkdirSync(join(root, 'exports'));
		writeFileSync(join(root, 'fuzz/fuzz_targets/anlz.rs'), '// paths: crates/loclib-anlz/src/**\n');
		writeFileSync(join(root, 'fuzz/fuzz_targets/wav.rs'), '// paths: crates/locallibrary/src/recordings.rs\n');
		writeFileSync(
			join(root, 'exports/anlz.json'),
			JSON.stringify(exported(file(`${root}/crates/loclib-anlz/src/lib.rs`, 4, 3)))
		);
		writeFileSync(join(root, 'exports/notes.txt'), 'not an export');
	});

	afterEach(() => rmSync(root, { recursive: true, force: true }));

	const read = (name: string) => readFileSync(join(root, 'out', name), 'utf8');

	it('writes the numbers, the table and the drops, and warns of each drop', () => {
		writeFileSync(join(root, 'last.json'), JSON.stringify({ anlz: all(80) }));

		const outcome = run(['report', join(root, 'exports'), join(root, 'last.json'), join(root, 'out')], root);

		expect(outcome).toEqual({
			lines: [
				'::warning title=Fuzz coverage dropped::anlz: lines 75% (was 80%), functions 50% (was 80%), regions 37.5% (was 80%)'
			],
			ok: true
		});
		expect(JSON.parse(read('coverage.json'))).toEqual({ anlz: { lines: 75, functions: 50, regions: 37.5 } });
		expect(read('coverage.md')).toContain(
			"| anlz | 75% (-5) | 50% (-30) | 37.5% (-42.5) |\n\nDropped since main's last weekly run:\n\n- anlz: lines 75%"
		);
		expect(read('drops.md')).toBe('- anlz: lines 75% (was 80%), functions 50% (was 80%), regions 37.5% (was 80%)\n');
	});

	it('measures the files the coverage lines name, not the rest of the paths', () => {
		writeFileSync(
			join(root, 'fuzz/fuzz_targets/anlz.rs'),
			'// paths: crates/loclib-anlz/src/**\n// coverage: crates/loclib-anlz/src/grid.rs\n'
		);
		writeFileSync(
			join(root, 'exports/anlz.json'),
			JSON.stringify(
				exported(
					file(`${root}/crates/loclib-anlz/src/lib.rs`, 100, 0),
					file(`${root}/crates/loclib-anlz/src/grid.rs`, 4, 4)
				)
			)
		);
		run(['report', join(root, 'exports'), join(root, 'missing.json'), join(root, 'out')], root);

		expect(JSON.parse(read('coverage.json'))).toEqual({ anlz: { lines: 100, functions: 50, regions: 50 } });
	});

	it('compares with nothing when there are no numbers from main', () => {
		expect(run(['report', join(root, 'exports'), join(root, 'missing.json'), join(root, 'out')], root)).toEqual({
			lines: [],
			ok: true
		});
		expect(read('coverage.md')).toMatch(/\| anlz \| 75% \| 50% \| 37\.5% \|\n$/);
		expect(read('drops.md')).toBe('');
	});

	it('fails on an export of a target that has gone', () => {
		writeFileSync(join(root, 'exports/gone.json'), JSON.stringify(exported()));

		expect(() => run(['report', join(root, 'exports'), join(root, 'last.json'), join(root, 'out')], root)).toThrow(
			'gone.json: no target gone in fuzz/fuzz_targets/'
		);
	});

	it.each([[['report', 'exports', 'last.json']], [['report', 'a', 'b', 'c', 'd']], [['select', 'a', 'b', 'c']]])(
		'gives the usage for %j',
		(args) => {
			expect(run(args, root)).toEqual({ lines: ['usage: cli.ts report <exports> <last.json> <out>'], ok: false });
		}
	);
});
