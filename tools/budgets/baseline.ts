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

/** Whether any of `files` is one `paths` select: it matches a glob and none of the `!` ones, as with `some-with-excludes`. */
export function selects(paths: string[], files: string[]): boolean {
	const glob = (patterns: string[]) => picomatch(patterns, { dot: true });
	const include = glob(paths.filter((path) => !path.startsWith('!')));
	const exclude = glob(paths.flatMap((path) => (path.startsWith('!') ? [path.slice(1)] : [])));

	return files.some((file) => include(file) && !exclude(file));
}

// How far back `baseline` looks: each CI run keeps its artifacts 14 days, and main rarely goes this
// many commits without one that changes what a budget measures.
const DEPTH = 20;

const git = (...args: string[]) => execFileSync('git', args, { encoding: 'utf8' }).trim();

const gh = (...args: string[]) => execFileSync('gh', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });

// The events whose runs measure the commit they're of. A pull request's run measures its merge
// commit, which holds its base branch as it was then, so its artifact isn't its head's numbers.
const MEASURES_ITS_COMMIT = new Set(['push', 'merge_group']);

/** Downloads `artifact` from a push or merge queue run of `sha` into `dir`: whether one had it. */
function downloaded(sha: string, artifact: string, dir: string): boolean {
	const listed = gh('run', 'list', '--workflow', 'ci.yml', '--commit', sha, '--json', 'databaseId,event');
	// SAFETY: `gh run list --json databaseId,event` prints an array of objects with those two fields.
	const runs = JSON.parse(listed) as { databaseId: number; event: string }[];

	return runs.some(({ databaseId, event }) => {
		if (!MEASURES_ITS_COMMIT.has(event)) return false;

		try {
			gh('run', 'download', String(databaseId), '--name', artifact, '--dir', dir);

			return true;
		} catch {
			return false;
		}
	});
}

function baseline(sha: string, filter: string, artifact: string, dir: string): Outcome {
	const paths = filterPaths(filter);
	const commits = git('rev-list', '--first-parent', `--max-count=${DEPTH}`, sha).split('\n');

	for (const commit of commits) {
		const changed = git('diff', '--name-only', commit, sha).split('\n');

		if (selects(paths, changed)) break;

		if (downloaded(commit, artifact, dir)) return { lines: ['found=true', `from=${commit}`], ok: true };
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
