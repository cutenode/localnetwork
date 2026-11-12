import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { type Budget, type Measured, loosened, problems, ratcheted, run, undeclared } from './budgets.ts';

const budget = (limit: number | null, tolerance_percent = 1): Budget => ({
	$schema: '../budget.schema.json',
	job: 'pull-request',
	tolerance_percent,
	metrics: { binary: { limit, unit: 'bytes', measures: 'The release binary.' } }
});

describe('problems', () => {
	it('passes a measurement within the tolerance of its limit, either way', () => {
		expect(problems('size', budget(1000), { binary: 1010 })).toEqual([]);
		expect(problems('size', budget(1000), { binary: 990 })).toEqual([]);
	});
	it('fails one over the limit and its tolerance', () => {
		expect(problems('size', budget(1000), { binary: 1011 })).toEqual([
			'size/binary: 1011 bytes is over its limit of 1000'
		]);
	});
	it('fails one that beats the limit by more than the tolerance, until the limit comes down', () => {
		expect(problems('size', budget(1000), { binary: 989 })).toEqual([
			'size/binary: 989 bytes beats its limit of 1000: run the ratchet and commit it'
		]);
	});
	it('fails a metric nothing measured, and a measurement with no budget', () => {
		expect(problems('size', budget(1000), { other: 1 })).toEqual([
			'size/binary: not measured',
			'size/other: measured, but has no budget'
		]);
	});
	it('passes any number for a metric with no limit yet', () => {
		expect(problems('size', budget(null), { binary: 5 })).toEqual([]);
	});
});

describe('ratcheted', () => {
	it('lowers a limit to the measurement', () => {
		expect(ratcheted(budget(1000), { binary: 900 }).metrics.binary?.limit).toBe(900);
	});
	it('never raises one', () => {
		expect(ratcheted(budget(1000), { binary: 1100 })).toEqual(budget(1000));
	});
	it('sets a limit a metric lacked', () => {
		expect(ratcheted(budget(null), { binary: 900 }).metrics.binary?.limit).toBe(900);
	});
	it('leaves a metric nothing measured alone', () => {
		expect(ratcheted(budget(1000), {})).toEqual(budget(1000));
	});
});

describe('loosened', () => {
	it('names a raised limit, a limit dropped or removed, and a wider tolerance', () => {
		expect(loosened('size', budget(1000), budget(1001))).toEqual(['size/binary']);
		expect(loosened('size', budget(1000), budget(null))).toEqual(['size/binary']);
		expect(loosened('size', budget(1000), { ...budget(1000), metrics: {} })).toEqual(['size/binary']);
		expect(loosened('size', budget(1000), budget(1000, 2))).toEqual(['size/*']);
	});
	it('names every metric of an area that went', () => {
		expect(loosened('size', budget(1000), undefined)).toEqual(['size/binary']);
	});
	it('takes a lowered limit, a first limit and a narrower tolerance', () => {
		expect(loosened('size', budget(1000), budget(999))).toEqual([]);
		expect(loosened('size', budget(null), budget(5))).toEqual([]);
		expect(loosened('size', budget(1000, 2), budget(1000, 1))).toEqual([]);
	});
});

describe('undeclared', () => {
	it('wants a line naming the metric and why', () => {
		const body = 'Text.\n\nbudget-raise: size/binary: the new decoder needs it\r\nbudget-raise: size/other:\n';
		expect(undeclared(['size/binary', 'size/other', 'size/third'], body)).toEqual(['size/other', 'size/third']);
	});
});

describe('the commands', () => {
	const start = process.cwd();
	let dir: string;

	const git = (...args: string[]) =>
		execFileSync('git', ['-c', 'commit.gpgsign=false', '-c', 'user.name=t', '-c', 'user.email=t@t', ...args]);

	const write = (name: string, body: Budget | Measured) => writeFileSync(name, `${JSON.stringify(body, null, '\t')}\n`);

	const commitBase = () => {
		git('init', '-q');
		git('add', '.');
		git('commit', '-qm', 'base');
	};

	beforeEach(() => {
		dir = mkdtempSync(join(tmpdir(), 'budgets-'));
		process.chdir(dir);
		mkdirSync('benchmarks/budgets', { recursive: true });
		mkdirSync('measured');
		write('benchmarks/budgets/size.json', budget(1000));
	});
	afterEach(() => {
		process.chdir(start);
		rmSync(dir, { recursive: true });
	});

	it('check fails on a budget a measurement breaks', () => {
		write('measured/size.json', { binary: 2000 });
		expect(run(['check', 'measured', 'pull-request'])).toEqual({
			lines: ['size/binary: 2000 bytes is over its limit of 1000'],
			ok: false
		});
	});
	it('check passes what holds', () => {
		write('measured/size.json', { binary: 1000 });
		expect(run(['check', 'measured', 'pull-request'])).toEqual({ lines: [], ok: true });
	});
	it('check fails an area of the job that nothing measured', () => {
		expect(run(['check', 'measured', 'pull-request'])).toEqual({
			lines: ['size: nothing measured, no measured/size.json'],
			ok: false
		});
	});
	it('leaves the areas of other jobs alone', () => {
		write('measured/size.json', { binary: 2000 });
		expect(run(['check', 'measured', 'weekly'])).toEqual({ lines: [], ok: true });
		expect(run(['ratchet', 'measured', 'weekly'])).toEqual({ lines: [], ok: true });
	});
	it('ratchet skips an area nothing measured', () => {
		expect(run(['ratchet', 'measured', 'pull-request'])).toEqual({ lines: [], ok: true });
	});
	it('ratchet writes what it lowers, and running it again changes nothing', () => {
		write('measured/size.json', { binary: 900 });
		expect(run(['ratchet', 'measured', 'pull-request']).lines).toEqual(['lowered benchmarks/budgets/size.json']);
		const once = readFileSync('benchmarks/budgets/size.json', 'utf8');
		expect(JSON.parse(once)).toEqual(budget(900));
		expect(run(['ratchet', 'measured', 'pull-request'])).toEqual({ lines: [], ok: true });
		expect(readFileSync('benchmarks/budgets/size.json', 'utf8')).toBe(once);
	});
	it('raises fails on a raise the body leaves out, and passes it declared', () => {
		commitBase();
		write('benchmarks/budgets/size.json', budget(2000));
		writeFileSync('body.md', 'No mention.\n');
		expect(run(['raises', 'HEAD', 'body.md'])).toEqual({
			lines: ["size/binary: loosened: add a line `budget-raise: size/binary: <why>` to the pull request's body"],
			ok: false
		});
		writeFileSync('body.md', 'budget-raise: size/binary: a new decoder\n');
		expect(run(['raises', 'HEAD', 'body.md'])).toEqual({ lines: [], ok: true });
	});
	it('raises counts a budget file taken away', () => {
		commitBase();
		rmSync('benchmarks/budgets/size.json');
		writeFileSync('body.md', '');
		expect(run(['raises', 'HEAD', 'body.md']).ok).toBe(false);
	});
	it.each([[[]], [['check', 'measured']], [['ratchet', 'a', 'b', 'c']], [['raises', 'HEAD']], [['other', 'a', 'b']]])(
		'%j prints how to use it',
		(args) => {
			expect(run(args).ok).toBe(false);
		}
	);
});
