import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { type Budget, type Measured, compared, declared, loosened, problems, run, table } from './budgets.ts';

const budget = (allowance = 10): Budget => ({
	$schema: '../budget.schema.json',
	job: 'pull-request',
	metrics: { binary: { allowance, unit: 'bytes', measures: 'The release binary.' } }
});

const names: Budget = {
	$schema: '../budget.schema.json',
	job: 'pull-request',
	metrics: { cli: { allowance: 0, unit: 'packages', measures: 'Packages the CLI links.' } }
};

describe('compared', () => {
	it('gives each metric its growth over the base', () => {
		expect(compared('size', budget(), { binary: 1000 }, { binary: 1004 })).toEqual([
			{ metric: 'size/binary', unit: 'bytes', base: 1000, head: 1004, growth: 4, added: [], allowance: 10 }
		]);
	});
	it('counts the names a list gained, not the ones it lost', () => {
		expect(compared('deps', names, { cli: ['a', 'b'] }, { cli: ['b', 'c', 'd'] })[0]?.growth).toBe(2);
	});
	it('gives no growth without a base, or between a count and a list', () => {
		expect(compared('size', budget(), undefined, { binary: 5 })[0]?.growth).toBeUndefined();
		expect(compared('size', budget(), {}, { binary: 5 })[0]?.growth).toBeUndefined();
		expect(compared('size', budget(), { binary: ['a'] }, { binary: 5 })[0]?.growth).toBe(0);
	});
});

const none = new Map<string, number>();

describe('problems', () => {
	const rows = (base: Measured | undefined, head: Measured) => compared('size', budget(), base, head);

	it('passes growth up to the allowance, and any shrinking', () => {
		expect(problems(rows({ binary: 1000 }, { binary: 1010 }), 1, none)).toEqual([]);
		expect(problems(rows({ binary: 1000 }, { binary: 10 }), 1, none)).toEqual([]);
	});
	it('fails one unit over the allowance', () => {
		expect(problems(rows({ binary: 1000 }, { binary: 1011 }), 1, none)).toEqual([
			"size/binary: 11 bytes more than its base, 1 over what's allowed: if it's meant, add a commit with the trailer `Budget-Raise: size/binary +1: <why>`"
		]);
	});
	it("allows each of a merge queue group's pull requests its allowance", () => {
		expect(problems(rows({ binary: 1000 }, { binary: 1020 }), 2, none)).toEqual([]);
		expect(problems(rows({ binary: 1000 }, { binary: 1021 }), 2, none)).toHaveLength(1);
	});
	it('allows a declared raise on top of the allowance, and asks for what it still lacks', () => {
		const raised = new Map([['size/binary', 100]]);

		expect(problems(rows({ binary: 1000 }, { binary: 1110 }), 1, raised)).toEqual([]);
		expect(problems(rows({ binary: 1000 }, { binary: 1115 }), 1, raised)).toEqual([
			"size/binary: 115 bytes more than its base, 5 over what's allowed: if it's meant, add a commit with the trailer `Budget-Raise: size/binary +5: <why>`"
		]);
	});
	it('passes a metric with no base, and fails one nothing measured', () => {
		expect(problems(rows(undefined, { binary: 9000 }), 1, none)).toEqual([]);
		expect(problems(rows({ binary: 1 }, {}), 1, none)).toEqual(['size/binary: not measured']);
	});
	it('names the packages a list gained', () => {
		const grown = compared('deps', names, { cli: ['a'] }, { cli: ['a', 'b', 'c'] });
		expect(problems(grown, 1, none)).toEqual([
			"deps/cli: b, c added, 2 over what's allowed: if it's meant, add a commit with the trailer `Budget-Raise: deps/cli +2: <why>`"
		]);
	});
});

describe('declared', () => {
	it('sums the trailers of each metric, as git log prints them', () => {
		const log =
			'Budget-Raise: size/binary +100: a decoder\n\nbudget-raise: size/binary +20: its tables\r\nBudget-Raise: deps/cli +1: rustls\n';

		expect(declared(log, '2026-12-01')).toEqual(
			new Map([
				['size/binary', 120],
				['deps/cli', 1]
			])
		);
	});
	it('takes no trailer without an amount or a reason', () => {
		expect(
			declared('Budget-Raise: size/binary: a decoder\nBudget-Raise: size/binary +5:\nBudget-Raise: size/binary 5: x\n')
		).toEqual(none);
	});
	it('lets a body line allow any growth, until the date', () => {
		const body = 'Text.\n\n budget-raise: size/binary: the new decoder needs it\r\nbudget-raise: size/other:\n';

		expect(declared(body, '2026-11-30')).toEqual(new Map([['size/binary', Infinity]]));
		expect(declared(body, '2026-12-01')).toEqual(none);
	});
});

describe('table', () => {
	it('shows counts, lists and a missing base', () => {
		const rows = [
			...compared('size', budget(), { binary: 1000 }, { binary: 1004 }),
			...compared('size', budget(), { binary: 1000 }, { binary: 990 }),
			...compared('deps', names, undefined, { cli: ['a', 'b'] })
		];

		expect(table(rows)).toEqual([
			'| Metric | Base | Head | Growth | Allowance |',
			'| --- | --- | --- | --- | --- |',
			'| size/binary | 1000 | 1004 | +4 | 10 bytes |',
			'| size/binary | 1000 | 990 | -10 | 10 bytes |',
			'| deps/cli |  | 2 names | no base | 0 packages |'
		]);
	});
});

describe('loosened', () => {
	it('names a widened allowance, and a metric or area that went', () => {
		expect(loosened('size', budget(10), budget(11))).toEqual(['size/binary']);
		expect(loosened('size', budget(10), { ...budget(10), metrics: {} })).toEqual(['size/binary']);
		expect(loosened('size', budget(10), undefined)).toEqual(['size/binary']);
	});
	it('takes a narrower or equal allowance', () => {
		expect(loosened('size', budget(10), budget(9))).toEqual([]);
		expect(loosened('size', budget(10), budget(10))).toEqual([]);
	});
});

describe('the commands', () => {
	const start = process.cwd();
	let dir: string;

	const git = (...args: string[]) =>
		execFileSync('git', ['-c', 'commit.gpgsign=false', '-c', 'user.name=t', '-c', 'user.email=t@t', ...args], {
			encoding: 'utf8'
		}).trim();

	const write = (name: string, body: Budget | Measured) => writeFileSync(name, `${JSON.stringify(body, null, '\t')}\n`);

	const commit = (message: string) => {
		git('add', '.');
		git('commit', '-qm', message);

		return git('rev-parse', 'HEAD');
	};

	beforeEach(() => {
		dir = mkdtempSync(join(tmpdir(), 'budgets-'));
		process.chdir(dir);
		mkdirSync('benchmarks/budgets', { recursive: true });
		mkdirSync('base');
		mkdirSync('head');
		write('benchmarks/budgets/size.json', budget());
		writeFileSync('raises.md', '');
	});
	afterEach(() => {
		process.chdir(start);
		rmSync(dir, { recursive: true });
	});

	describe('check', () => {
		it('fails growth past the allowance, with the table above', () => {
			write('base/size.json', { binary: 1000 });
			write('head/size.json', { binary: 1011 });
			const { lines, ok } = run(['check', 'pull-request', 'base', 'head', 'raises.md']);
			expect(ok).toBe(false);
			expect(lines.slice(2)).toEqual([
				'| size/binary | 1000 | 1011 | +11 | 10 bytes |',
				'',
				"size/binary: 11 bytes more than its base, 1 over what's allowed: if it's meant, add a commit with the trailer `Budget-Raise: size/binary +1: <why>`"
			]);
		});
		it('passes growth within the allowance times the entries, and a declared raise', () => {
			write('base/size.json', { binary: 1000 });
			write('head/size.json', { binary: 1020 });
			expect(run(['check', 'pull-request', 'base', 'head', 'raises.md', '2']).ok).toBe(true);
			expect(run(['check', 'pull-request', 'base', 'head', 'raises.md']).ok).toBe(false);
			writeFileSync('raises.md', 'Budget-Raise: size/binary +10: a new decoder\n');
			expect(run(['check', 'pull-request', 'base', 'head', 'raises.md']).ok).toBe(true);
		});
		it('passes anything with no base measured', () => {
			write('head/size.json', { binary: 9999 });
			expect(run(['check', 'pull-request', 'none', 'head', 'raises.md']).ok).toBe(true);
		});
		it('fails an area nothing measured, and a metric with no budget', () => {
			expect(run(['check', 'pull-request', 'base', 'head', 'raises.md']).lines.at(-1)).toBe(
				'size: nothing measured, no head/size.json'
			);
			write('head/size.json', { binary: 1, other: 2 });
			expect(run(['check', 'pull-request', 'base', 'head', 'raises.md']).lines.at(-1)).toBe(
				'size/other: measured, but has no budget'
			);
		});
		it('leaves the areas of other jobs alone', () => {
			expect(run(['check', 'weekly', 'base', 'head', 'raises.md']).ok).toBe(true);
		});
	});

	describe('raises', () => {
		it('fails a widened allowance the body leaves out, and passes it declared', () => {
			git('init', '-q');
			commit('base');
			write('benchmarks/budgets/size.json', budget(20));
			expect(run(['raises', 'HEAD', 'raises.md'])).toEqual({
				lines: [
					"size/binary: loosened: if it's meant, add a commit with the trailer `Budget-Raise: size/binary +0: <why>`"
				],
				ok: false
			});
			writeFileSync('raises.md', 'Budget-Raise: size/binary +0: a noisier metric\n');
			expect(run(['raises', 'HEAD', 'raises.md'])).toEqual({ lines: [], ok: true });
		});
		it('counts a budget file taken away', () => {
			git('init', '-q');
			commit('base');
			rmSync('benchmarks/budgets/size.json');
			expect(run(['raises', 'HEAD', 'raises.md']).ok).toBe(false);
		});
	});

	it.each([
		[[]],
		[['check', 'pull-request', 'base', 'head']],
		[['check', 'pull-request', 'base', 'head', 'raises.md', '0']],
		[['check', 'pull-request', 'base', 'head', 'raises.md', '1.5']],
		[['raises', 'HEAD']],
		[['other', 'a', 'b']]
	])('%j prints how to use it', (args) => {
		expect(run(args).ok).toBe(false);
	});
});
