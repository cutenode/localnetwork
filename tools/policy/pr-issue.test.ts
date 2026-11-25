import { spawnSync } from 'node:child_process';
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { parse } from 'yaml';

const repo = resolve(import.meta.dirname, '../..');

const workflow = parse(readFileSync(resolve(repo, '.github/workflows/pr-issue.yml'), 'utf8'));

const template = readFileSync(resolve(repo, '.github/pull_request_template.md'), 'utf8');

const step = workflow.jobs.issue.steps[0];

// GitHub's API, as the step asks it: how many issues the pull request closes, which is CLOSES.
const bin = mkdtempSync(join(tmpdir(), 'pr-issue-'));

afterAll(() => rmSync(bin, { recursive: true, force: true }));

writeFileSync(
	join(bin, 'gh'),
	'#!/bin/bash\n[[ "$*" == *closingIssuesReferences* && "$*" == *number=7* ]] || exit 1\necho "$CLOSES"\n'
);

chmodSync(join(bin, 'gh'), 0o755);

// Runs the workflow's own step as the runner does, for a pull request with this description and
// branch, that GitHub says closes `closes` issues. GitHub knows only pull request 7.
const passes = (BODY: string, BRANCH = 'bnb/some-feature', closes = 0, NUMBER = '7'): boolean =>
	spawnSync('bash', ['--noprofile', '--norc', '-eo', 'pipefail', '-c', step.run], {
		env: {
			PATH: `${bin}:${process.env.PATH}`,
			BODY,
			BRANCH,
			NUMBER,
			GH_REPO: 'cutenode/localnetwork',
			QUERY: step.env.QUERY,
			CLOSES: String(closes)
		},
		stdio: 'ignore'
	}).status === 0;

describe('the PR issue check', () => {
	it.each([
		'Fixes LOC-12',
		'part of loc-9',
		'Closes LOC-3',
		'Resolved LOC-3',
		'Implements LOC-3',
		'Linear issue LOC-3',
		'Refs LOC-3',
		'Contributes to LOC-3',
		'Toward LOC-3',
		'Fixes LOC-795, LOC-625, LOC-786',
		'Some context.\n\nPart of LOC-44'
	])('passes on %s', (body) => {
		expect(passes(body)).toBe(true);
	});

	it('passes on a bnb/loc-123 branch with an empty description', () => {
		expect(passes('', 'bnb/loc-44-foo')).toBe(true);
	});

	it('passes when GitHub says the pull request closes an issue', () => {
		expect(passes('Fixes #301', 'bnb/some-feature', 1)).toBe(true);
	});

	it.each([
		['an empty description', ''],
		['the unedited template', template],
		['a reference only in a comment', 'Nothing yet. <!-- Fixes LOC-12 -->'],
		['a comment across lines', '<!--\nFixes LOC-12\n-->'],
		['an issue with no magic word', 'LOC-12'],
		['a word Linear only relates with', 'Related to LOC-12'],
		['words between the magic word and the issue', 'Fixes the reader LOC-12 added'],
		['the end of a longer word', 'Prefixes LOC-12'],
		["an issue GitHub says the pull request doesn't close", 'Fixes #301']
	])('fails on %s', (_, body) => {
		expect(passes(body)).toBe(false);
	});

	it.each(['bnb/feature-loc-44', 'loc-44-foo', 'bnb/loc-foo'])('fails on the branch %s', (branch) => {
		expect(passes('', branch)).toBe(false);
	});

	it('fails when GitHub has no answer', () => {
		expect(passes('Fixes #301', 'bnb/some-feature', 1, '8')).toBe(false);
	});
});
