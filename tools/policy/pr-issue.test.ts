import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { parse } from 'yaml';

const repo = resolve(import.meta.dirname, '../..');

const workflow = parse(readFileSync(resolve(repo, '.github/workflows/pr-issue.yml'), 'utf8'));

const template = readFileSync(resolve(repo, '.github/pull_request_template.md'), 'utf8');

const script: string = workflow.jobs.issue.steps[0].run;

// Runs the workflow's own step as the runner does, for a pull request with this description and branch.
const passes = (BODY: string, BRANCH = 'bnb/some-feature'): boolean =>
	spawnSync('bash', ['-e', '-c', script], {
		env: { PATH: process.env.PATH, BODY, BRANCH, REPO: 'cutenode/localnetwork' },
		stdio: 'ignore'
	}).status === 0;

describe('the PR issue check', () => {
	it.each([
		'Fixes LOC-12',
		'part of loc-9',
		'Fixes #301',
		'(#301)',
		'See https://github.com/cutenode/localnetwork/issues/301'
	])('passes on %s', (body) => {
		expect(passes(body)).toBe(true);
	});

	it('passes on a bnb/loc-123 branch with an empty description', () => {
		expect(passes('', 'bnb/loc-44-foo')).toBe(true);
	});

	it.each([
		['an empty description', ''],
		['the unedited template', template],
		['a reference only in a comment', 'Nothing yet. <!-- Fixes LOC-12 -->'],
		['a comment across lines', '<!--\nFixes #301\n-->'],
		['the end of a longer word', 'ALLOC-12 and issue#12'],
		['an issue in another repository', 'https://github.com/cutenode/other/issues/301'],
		['a pull request URL', 'https://github.com/cutenode/localnetwork/pull/301']
	])('fails on %s', (_, body) => {
		expect(passes(body)).toBe(false);
	});

	it.each(['bnb/feature-loc-44', 'loc-44-foo', 'bnb/loc-foo'])('fails on the branch %s', (branch) => {
		expect(passes('', branch)).toBe(false);
	});
});
