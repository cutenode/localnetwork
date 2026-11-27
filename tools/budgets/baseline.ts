// Where a pull request's or merge queue group's base measurements come from: the artifact of the CI
// run of that commit (a merge queue group's or a push's), or of the nearest commit before it on main's first-parent line that ci.yml's
// paths filter for the job says is the same, since each group's head becomes main. Loaded only for
// this command, as it needs the root's dependencies, which the jobs that only check don't install.
//
//   node tools/budgets/cli.ts baseline <sha> <filter> <artifact> <dir>
//       downloads <artifact> into <dir>; prints `found=true` and `from=<commit>`, or `found=false`
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import picomatch from 'picomatch';
import { parse } from 'yaml';
import type { Outcome } from './budgets.ts';

/** The paths ci.yml's `changes` job gives `filter`, as dorny/paths-filter reads them. */
export function filterPaths(filter: string): string[] {
	// SAFETY: ci.yml is the repository's own workflow, and tools/policy/workflows.rego checks its shape.
	const workflow = parse(readFileSync('.github/workflows/ci.yml', 'utf8')) as {
		jobs: { changes: { steps: { id?: string; with: { filters: string } }[] } };
	};

	const filters = workflow.jobs.changes.steps.flatMap((step) => (step.id === 'filter' ? [step.with.filters] : []));
	// SAFETY: the filter step's `filters` is a YAML map of filter names to lists of globs.
	const paths = (parse(filters.join('\n')) as Record<string, string[] | undefined>)[filter];

	if (paths === undefined) throw new Error(`ci.yml has no ${filter} paths filter`);

	return paths;
}

/** Whether `paths` select a file: it matches a glob and none of the `!` ones, as with `some-with-excludes`. Compiled once, for many files. */
export function selecting(paths: string[]): (file: string) => boolean {
	const glob = (patterns: string[]) => picomatch(patterns, { dot: true });
	const include = glob(paths.filter((path) => !path.startsWith('!')));
	const exclude = glob(paths.flatMap((path) => (path.startsWith('!') ? [path.slice(1)] : [])));

	return (file) => include(file) && !exclude(file);
}

/** The first of `files` that `paths` select (see [`selecting`]). */
export function selected(paths: string[], files: string[]): string | undefined {
	return files.find(selecting(paths));
}

const git = (...args: string[]) => execFileSync('git', args, { encoding: 'utf8' }).trim();

const gh = (...args: string[]) => execFileSync('gh', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });

// The events whose runs measure the commit they're of. A pull request's run measures its merge
// commit, which holds its base branch as it was then, so its artifact isn't its head's numbers.
const MEASURES_ITS_COMMIT = new Set(['push', 'merge_group']);

/** The runs that kept an unexpired `artifact`, newest first, by the commit each was of: one listing, not a request per commit. */
function runsByCommit(artifact: string): Map<string, number[]> {
	const listed = gh(
		'api',
		'--paginate',
		'--slurp',
		`repos/{owner}/{repo}/actions/artifacts?name=${artifact}&per_page=100`
	);

	// SAFETY: `--slurp` gives the listing's pages as an array, each with GitHub's `artifacts` array.
	const pages = JSON.parse(listed) as {
		artifacts: { expired: boolean; workflow_run: { id: number; head_sha: string } }[];
	}[];

	const runs = new Map<string, number[]>();

	for (const { expired, workflow_run: from } of pages.flatMap((page) => page.artifacts)) {
		if (!expired) runs.set(from.head_sha, [...(runs.get(from.head_sha) ?? []), from.id]);
	}

	return runs;
}

/** Downloads `artifact` from the first of `runs` that's a push or merge queue run into `dir`: whether one had it. */
function downloaded(runs: number[], artifact: string, dir: string): boolean {
	return runs.some((id) => {
		const event = gh('api', `repos/{owner}/{repo}/actions/runs/${id}`, '--jq', '.event').trim();

		if (!MEASURES_ITS_COMMIT.has(event)) return false;

		try {
			gh('run', 'download', String(id), '--name', artifact, '--dir', dir);

			return true;
		} catch {
			return false;
		}
	});
}

// Back from `sha` along main's first parents, each commit with the files it changed, until one changed
// what the job measures: that one's numbers are still `sha`'s, the ones before it aren't. Looking costs
// nothing per commit, so it goes as far as the checkout's history does.
function baseline(sha: string, filter: string, artifact: string, dir: string): Outcome {
	const paths = filterPaths(filter);
	const runs = runsByCommit(artifact);
	const commits = git('log', '--first-parent', '--format=%x00%H', '--name-only', sha).split('\0').slice(1);

	for (const [commit = '', ...changed] of commits.map((lines) => lines.trim().split('\n'))) {
		if (downloaded(runs.get(commit) ?? [], artifact, dir)) return { lines: ['found=true', `from=${commit}`], ok: true };

		if (selected(paths, changed) !== undefined) break;
	}

	return { lines: ['found=false'], ok: true };
}

/** Runs `baseline <sha> <filter> <artifact> <dir>`. */
export function run([command, ...args]: string[]): Outcome {
	if (command !== 'baseline' || args.length !== 4) {
		return { lines: ['usage: cli.ts baseline <sha> <filter> <artifact> <dir>'], ok: false };
	}

	// SAFETY: the length is checked just above.
	const [sha, filter, artifact, dir] = args as [string, string, string, string];

	return baseline(sha, filter, artifact, dir);
}
