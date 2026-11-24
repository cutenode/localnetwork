// oxfmt and oxlint walk each app's folder themselves, honouring ignore files. A rule that hid a whole
// checkout (as a `.claude/` line in a global git ignore was suspected of, for worktrees kept under
// `.claude/worktrees/`) would let format and lint pass having looked at nothing. These pin the two things
// that stop that: each app's walk, and each of the root's own, finds its files, and a walk that finds none fails.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';

const bin = (tool: string) => join(import.meta.dirname, '../../node_modules/.bin', tool);

const run = (tool: string, args: string[], cwd: string) => spawnSync(bin(tool), args, { cwd, encoding: 'utf8' });

// How many files oxfmt checks with `args` in `cwd`.
const formatted = (args: string[], cwd: string) => {
	const { status, stdout, stderr } = run('oxfmt', ['--check', ...args], cwd);
	// 0 is formatted and 1 is not: either way it looked. 2 is "no files".
	expect(status, stderr).not.toBe(2);

	return Number(/ on (\d+) files/.exec(stdout + stderr)?.[1]);
};

// How many files oxlint lints with `args` in `cwd`.
const linted = (args: string[], cwd: string) => {
	const { stdout } = run('oxlint', ['--format=json', ...args], cwd);

	// SAFETY: oxlint's JSON report always has `number_of_files`; a missing one reads as NaN and fails.
	return (JSON.parse(stdout) as { number_of_files: number }).number_of_files;
};

const apps = ['apps/web', 'apps/localtoolbox', 'packages/atstore'];

// An app's walk is counted with every rule off and no JS plugins (an empty config): the files it takes
// come from the ignore files, not the rules, and loading the plugins took seconds a walk, past vitest's
// 5 s under a parallel run.
const scratch = mkdtempSync(join(tmpdir(), 'walks-config-'));

const noRules = join(scratch, 'oxlintrc.json');

writeFileSync(noRules, '{}');

afterAll(() => rmSync(scratch, { recursive: true, force: true }));

const root = join(import.meta.dirname, '../..');

describe.each(apps)('%s', (app) => {
	it('is walked by oxfmt', () => {
		expect(formatted([], join(root, app))).toBeGreaterThan(0);
	});

	it('is walked by oxlint', () => {
		expect(linted(['-c', noRules, '-A', 'all'], join(root, app))).toBeGreaterThan(0);
	});
});

// SAFETY: the root package.json always has scripts; a missing one reads as undefined and fails below.
const { scripts } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
	scripts: Record<string, string | undefined>;
};

// The root's scripts walk paths it names, so a script is run as written: the tool, then its words, with a
// quoted glob unquoted the way the shell would.
describe.each([
	{ script: 'format:tools', tool: 'oxfmt', walk: formatted },
	{ script: 'lexicons:format', tool: 'oxfmt', walk: formatted },
	{ script: 'lint:tools', tool: 'oxlint', walk: linted }
])("the root's $script", ({ script, tool, walk }) => {
	it(`is walked by ${tool}`, () => {
		const [command, ...args] = (scripts[script] ?? '').split(' ').map((word) => word.replace(/^'(.*)'$/, '$1'));
		expect(command).toBe(tool);
		expect(walk(args, root)).toBeGreaterThan(0);
	});
});

describe('a walk with nothing to check', () => {
	const empty = mkdtempSync(join(tmpdir(), 'walks-'));

	afterAll(() => rmSync(empty, { recursive: true, force: true }));

	it('fails oxfmt', () => {
		expect(run('oxfmt', ['--check'], empty).status).not.toBe(0);
	});

	it('fails oxlint', () => {
		expect(run('oxlint', [], empty).status).not.toBe(0);
	});
});
