// Which analysis codeql.yml uploads again in place of analysing, from its reuse step's own script, with
// GitHub stood in for.
import { execFileSync } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { parse } from 'yaml';

// SAFETY: codeql.yml is the repository's own workflow.
const codeql = parse(readFileSync('.github/workflows/codeql.yml', 'utf8')) as {
	jobs: { analyze: { steps: { id?: string; run?: string }[] } };
};

const script = codeql.jobs.analyze.steps.find(({ id }) => id === 'reuse')?.run ?? '';

type Analysis = { id: number; commit_sha: string; category: string; ref: string; error: string };

const MAIN = 'refs/heads/main';

const QUEUE = 'refs/heads/gh-readonly-queue/main/pr-765-fd22f6339';

const analysis = (id: number, change: Partial<Analysis> = {}): Analysis => ({
	id,
	commit_sha: 'base',
	category: '/language:rust',
	ref: MAIN,
	error: '',
	...change
});

describe('the reuse step', () => {
	let dir: string;

	beforeEach(() => {
		dir = mkdtempSync(join(tmpdir(), 'codeql-'));
	});
	afterEach(() => {
		rmSync(dir, { recursive: true });
	});

	/** The analysis the step reuses, from main's list, every ref's, and its second page. */
	const reuses = (lists: (Analysis[] | 'fails')[], env: Record<string, string> = {}) => {
		const [main, all, second] = ['main', 'all', 'second'].map((name) => join(dir, name));

		[main, all, second].forEach((path, i) => {
			rmSync(path, { force: true });

			if (lists[i] !== 'fails') writeFileSync(path, JSON.stringify(lists[i] ?? []));
		});
		// On an HTTP error, gh prints GitHub's answer and exits 1.
		writeFileSync(
			join(dir, 'gh'),
			`#!/bin/sh
fails() { echo '{"message":"Server Error"}'; exit 1; }
case "$*" in
  *sarif*analyses/9*) fails ;;
  *sarif*) echo "{}" ;;
  *ref=refs/heads/main*) cat "${main}" || fails ;;
  *page=2*) cat "${second}" || fails ;;
  *) cat "${all}" || fails ;;
esac
`
		);
		chmodSync(join(dir, 'gh'), 0o755);

		const log = execFileSync('bash', ['--noprofile', '--norc', '-eo', 'pipefail', '-c', script], {
			env: {
				...process.env,
				PATH: `${dir}:${process.env.PATH ?? ''}`,
				GH_REPO: 'cutenode/localnetwork',
				RUNNER_TEMP: dir,
				GITHUB_OUTPUT: join(dir, 'output'),
				EVENT: 'pull_request',
				SHA: 'own',
				UNCHANGED: 'true',
				BASE: 'base',
				LANGUAGE: 'rust',
				...env
			},
			encoding: 'utf8',
			stdio: ['ignore', 'pipe', 'ignore']
		});

		const reused = /Reusing analysis (\d+)/.exec(log)?.[1];
		expect(existsSync(join(dir, 'output'))).toBe(reused !== undefined);

		return reused;
	};

	it("reuses main's analysis of the base", () => {
		expect(reuses([[analysis(1)], [analysis(2, { ref: QUEUE })]])).toBe('1');
	});

	it("reuses the queue group's when main has none yet, on either page", () => {
		expect(reuses([[], [analysis(2, { ref: QUEUE })]])).toBe('2');
		rmSync(join(dir, 'output'));
		expect(reuses([[], [], [analysis(3, { ref: QUEUE })]])).toBe('3');
	});

	it.each([
		['of another commit', { commit_sha: 'other' }],
		['in another language', { category: '/language:actions' }],
		['that errored', { error: 'out of memory' }],
		["of a pull request's merge commit", { ref: 'refs/pull/7/merge' }],
		['of another branch', { ref: 'refs/heads/main-copy' }],
		["of another branch's queue", { ref: 'refs/heads/gh-readonly-queue/other/pr-1-a' }]
	])('analyses when all there is is one %s', (_, change) => {
		expect(reuses([[], [analysis(2, change)]])).toBeUndefined();
	});

	it('analyses when there is none, or the one there is cannot be read', () => {
		expect(reuses([[], [], []])).toBeUndefined();
		expect(reuses([[analysis(9)]])).toBeUndefined();
	});

	it.each([
		["main's list", ['fails', [analysis(2)], []]],
		["every ref's", [[analysis(1)], 'fails', []]],
		['its second page', [[analysis(1)], [], 'fails']]
	] satisfies [string, (Analysis[] | 'fails')[]][])('analyses when GitHub fails on %s', (_, lists) => {
		expect(reuses(lists)).toBeUndefined();
	});

	it("reuses the group's Rust analysis of a push's own commit, though it changed", () => {
		const own = analysis(4, { ref: QUEUE, commit_sha: 'own' });

		expect(reuses([[], [own]], { EVENT: 'push', UNCHANGED: 'false' })).toBe('4');
	});

	it('analyses a push whose JavaScript changed, whose caches pull requests start from', () => {
		const own = analysis(4, { ref: QUEUE, commit_sha: 'own', category: '/language:javascript-typescript' });
		const push = { EVENT: 'push', UNCHANGED: 'false', LANGUAGE: 'javascript-typescript' };

		expect(reuses([[own], [own]], push)).toBeUndefined();
	});

	it("falls back on the base for a push whose own commit has none, only for a language that didn't change", () => {
		expect(reuses([[analysis(1)], []], { EVENT: 'push' })).toBe('1');
		rmSync(join(dir, 'output'));
		expect(reuses([[analysis(1)], []], { EVENT: 'push', UNCHANGED: 'false' })).toBeUndefined();
	});

	it("never takes a pull request's own commit for its base", () => {
		expect(reuses([[analysis(1, { commit_sha: 'own' })], []])).toBeUndefined();
	});
});
