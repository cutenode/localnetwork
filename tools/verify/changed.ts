// `pnpm verify:changed`: the verify scripts for what the branch and the working tree change, picked
// as ci.yml's changes job picks CI's jobs: a script runs when one of its filters selects a changed file.
import { spawnSync } from 'node:child_process';
import { filterPaths, selected } from '../budgets/baseline.ts';
import { type Verdict, steps, verify } from './run.ts';

/** ci.yml's text job (typos and lychee, which verify:repo runs) has no filter: it runs on every change. */
export const EVERY_CHANGE = 'every change';

// The root verify's own steps, which the repo checks' policy and knip jobs run too. Each app's filter
// stands in for its `pnpm -r`.
const ROOT_STEPS = [...steps]
	.flatMap(([script, commands]) => (script === 'verify' ? commands : []))
	.filter((command) => !command.startsWith('pnpm -r '));

const SCRIPTS = {
	'pnpm -C apps/web verify': ['web'],
	'pnpm -C apps/localtoolbox verify': ['localtoolbox'],
	'pnpm -C packages/atstore verify': ['atstore'],
	...Object.fromEntries(ROOT_STEPS.map((command) => [command, ['repo_checks']])),
	// The workspace, the crates' READMEs (their doctests), the docs its tests read, and what dist plans from.
	'pnpm verify:rust': ['locallibrary', 'locallibrary_readmes', 'locallibrary_md', 'locallibrary_dist'],
	// Every change, which takes in repo_checks' files and repo_checks_md's.
	'pnpm verify:repo': [EVERY_CHANGE]
};

type Changed = { files: string[] } | { problem: string };

/** Each script, with the filters that pick it. */
type Scripts = Record<string, string[]>;

const paths = (filter: string): string[] => (filter === EVERY_CHANGE ? ['**'] : filterPaths(filter));

/** The scripts `files` call for, each with the first file that picked it. */
export const picks = (files: string[], scripts: Scripts = SCRIPTS) =>
	Object.entries(scripts).flatMap(([command, filters]) => {
		const file = filters.map((filter) => selected(paths(filter), files)).find((found) => found !== undefined);

		return file === undefined ? [] : [{ command, file }];
	});

const git = (cwd: string, ...args: string[]) => spawnSync('git', args, { cwd, encoding: 'utf8' });

/** What the branch changes from origin/main, and the working tree on top of it (new files too), from `cwd`, the root. */
export const changedFiles = (cwd: string): Changed => {
	const branch = git(cwd, 'diff', '--name-only', '-z', 'origin/main...');

	if (branch.status !== 0)
		return {
			problem: "verify:changed compares with origin/main, which this checkout doesn't have: git fetch origin main"
		};

	const listed = [
		branch,
		git(cwd, 'diff', '--name-only', '-z', 'HEAD'),
		git(cwd, 'ls-files', '--others', '--exclude-standard', '-z')
	];

	return { files: [...new Set(listed.flatMap(({ stdout }) => stdout.split('\0').filter(Boolean)))] };
};

/** Runs the scripts the changed files call for, and says which file picked each one. */
export const verifyChanged = (changed: Changed, scripts: Scripts = SCRIPTS): Verdict => {
	if ('problem' in changed) return { lines: [changed.problem], ok: false };

	const picked = picks(changed.files, scripts);

	if (picked.length === 0) return { lines: ['Nothing changed from origin/main: nothing to verify'], ok: true };

	const { lines, ok } = verify('verify:changed', new Map([['verify:changed', picked.map(({ command }) => command)]]));

	return { lines: [...picked.map(({ command, file }) => `${command}: picked by ${file}`), ...lines], ok };
};
