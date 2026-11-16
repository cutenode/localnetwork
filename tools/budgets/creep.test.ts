import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { Budget } from './budgets.ts';
import { type Point, creep, crept, report, run } from './creep.ts';

const size: Budget = {
	$schema: '../budget.schema.json',
	job: 'release',
	metrics: { binary: { allowance: 10, unit: 'bytes', measures: 'The release binary.' } }
};

const point = (sha: string, binary: number | undefined, prs: number[] = [], raised = 0): Point => ({
	sha,
	measured: binary === undefined ? {} : { size: { binary } },
	prs,
	raised: new Map(raised > 0 ? [['size/binary', raised]] : [])
});

describe('creep', () => {
	it('adds up growth between measured commits, declared up to each raise, by pull request, across one that measured nothing', () => {
		const points = [
			point('a', 1000),
			point('b', 1008, [1]),
			point('c', 1108, [2, 3], 90),
			point('d', 1100, [4], 50),
			point('e', undefined, [5], 5),
			point('f', 1109, [6])
		];

		expect(creep(points, [['size', size]])).toEqual([
			{
				metric: 'size/binary',
				unit: 'bytes',
				growth: 8 + 100 - 8 + 9,
				declared: 90 + 5,
				undeclared: 8 + 10 - 8 + 4,
				allowance: 10,
				shares: [
					['#2, #3', 100],
					['#5, #6', 9],
					['#1', 8]
				]
			}
		]);
	});
	it('names a commit with no pull request by its hash', () => {
		const [row] = creep([point('a', 1), point('0123456789ab', 5)], [['size', size]]);

		expect(row?.shares).toEqual([['012345678', 4]]);
	});
});

describe('crept', () => {
	const row = (undeclared: number, allowance: number) => ({
		metric: 'size/binary',
		unit: 'bytes',
		growth: undeclared,
		declared: 0,
		undeclared,
		allowance,
		shares: []
	});

	it('fails undeclared growth over five allowances', () => {
		expect(crept([row(50, 10)])).toEqual([]);
		expect(crept([row(51, 10)])).toEqual([
			'size/binary: 51 bytes of growth no Budget-Raise declared, over 5 allowances (50)'
		]);
	});
	it('fails any undeclared growth of a metric allowed none, and none of no growth', () => {
		expect(crept([row(1, 0)])).toHaveLength(1);
		expect(crept([row(0, 0)])).toEqual([]);
	});
});

describe('report', () => {
	it('shows the three pull requests that grew a metric most', () => {
		const rows = creep(
			[point('a', 0), point('b', 4, [1]), point('c', 7, [2]), point('d', 12, [3]), point('e', 13, [4])],
			[['size', size]]
		);

		expect(report(rows)).toEqual([
			'| Metric | Growth | Declared | Undeclared | Most from |',
			'| --- | --- | --- | --- | --- |',
			'| size/binary | 13 bytes | 0 | 13 | #3 +5, #1 +4, #2 +3 |'
		]);
	});
});

describe('the command', () => {
	const start = process.cwd();
	let dir: string;

	const git = (...args: string[]) =>
		execFileSync('git', ['-c', 'commit.gpgsign=false', '-c', 'user.name=t', '-c', 'user.email=t@t', ...args], {
			encoding: 'utf8'
		}).trim();

	const merge = (pr: number, trailer?: string) => {
		git('checkout', '-qb', `pr-${pr}`);
		git('commit', '-q', '--allow-empty', '-m', `feat: ${pr}`, ...(trailer === undefined ? [] : ['--trailer', trailer]));
		git('checkout', '-q', 'main');
		git('merge', '-q', '--no-ff', `pr-${pr}`, '-m', `Merge pull request #${pr} from cutenode/pr-${pr}`);

		return git('rev-parse', 'HEAD');
	};

	const measure = (order: number, sha: string, binary: number) => {
		mkdirSync(join('points', `${order}-${sha}`), { recursive: true });
		writeFileSync(join('points', `${order}-${sha}`, 'size.json'), JSON.stringify({ binary }));
	};

	beforeEach(() => {
		dir = mkdtempSync(join(tmpdir(), 'creep-'));
		process.chdir(dir);
		mkdirSync('benchmarks/budgets', { recursive: true });
		writeFileSync('benchmarks/budgets/size.json', JSON.stringify(size));
		git('init', '-qb', 'main');
		git('commit', '-q', '--allow-empty', '-m', 'start');
	});
	afterEach(() => {
		process.chdir(start);
		rmSync(dir, { recursive: true });
	});

	it("reads each step's pull requests and trailers from git, and fails creep past the cap", () => {
		measure(0, git('rev-parse', 'HEAD'), 1000);
		measure(1, merge(1, 'Budget-Raise: size/binary +200: a decoder'), 1200);
		measure(2, merge(2), 1260);

		const { lines, ok } = run(['creep', 'release', 'points']);

		expect(lines).toEqual([
			'| Metric | Growth | Declared | Undeclared | Most from |',
			'| --- | --- | --- | --- | --- |',
			'| size/binary | 260 bytes | 200 | 60 | #1 +200, #2 +60 |',
			'',
			'size/binary: 60 bytes of growth no Budget-Raise declared, over 5 allowances (50)'
		]);
		expect(ok).toBe(false);
	});
	it('passes with nothing measured, and leaves other jobs alone', () => {
		expect(run(['creep', 'release', 'points']).ok).toBe(true);
		measure(0, git('rev-parse', 'HEAD'), 1000);
		expect(run(['creep', 'web', 'points']).lines).toHaveLength(3);
	});
	it.each([[[]], [['creep', 'release']], [['check', 'a', 'b']]])('%j prints how to use it', (args) => {
		expect(run(args).ok).toBe(false);
	});
});
