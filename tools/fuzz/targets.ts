// Which cargo-fuzz targets a change reaches, so a pull request fuzzes and replays only those. Each
// target in fuzz/fuzz_targets/ starts with its paths, a `// paths: <glob>` line each (picomatch
// globs from the repository root, as ci.yml's filters have them): the target itself, its seeds and
// dictionary, and the code it fuzzes, named down to the modules where it goes through a big crate.
// Where a path is more than it fuzzes (a parser's callers), `// coverage: <glob>` lines name the part
// its coverage is measured over (report.ts). The tests, props and Markdown under those globs never
// count (EXCLUDED), and a change to what builds or runs every target counts for all of them
// (EVERY_TARGET).
//
//   node tools/fuzz/cli.ts select [<base>]   the targets the changes from <base> to HEAD reach, or every
//                                            target, separated by spaces
//   node tools/fuzz/cli.ts corpora <target>  the folders its fuzz-<target> alias reads, one a line
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { selected, selecting } from '../budgets/baseline.ts';
import type { Outcome } from '../coverage/coverage.ts';

const TARGETS = 'fuzz/fuzz_targets';

/**
 * What builds or runs every target on a pull request: their manifest and lockfile, the aliases, the
 * actions, this tool and the workflow with the fuzz jobs, as ci.yml's locallibrary_fuzz has them.
 */
export const EVERY_TARGET = [
	'fuzz/Cargo.toml',
	'fuzz/Cargo.lock',
	'.cargo/config.toml',
	'.github/actions/fuzz*/**',
	'.github/workflows/locallibrary-ci.yml',
	'tools/fuzz/**'
];

/** Left out of every target's paths, as ci.yml's locallibrary filters leave them out: the fixtures beside tests still count. */
export const EXCLUDED = ['!**/{tests,testing,test_util,screens,props}.rs', '!**/tests/**/*.rs', '!**/*.md'];

/** A target's globs: the changes that run it, and the files its coverage is measured over. */
export interface Target {
	paths: string[];
	coverage: string[];
}

const HEADER = /^\/\/ (paths|coverage): (\S+)$/;

/** The globs of a target's leading `// paths:` and `// coverage:` lines; with none of those, it's measured over its paths. */
export function headerOf(source: string): Target {
	const paths: string[] = [];
	const coverage: string[] = [];

	for (const line of source.split('\n')) {
		const [, kind, glob] = HEADER.exec(line) ?? [];

		if (glob === undefined) {
			if (/^\/\/ (paths|coverage):/.test(line)) throw new Error(`not one glob: ${line}`);

			break;
		}

		(kind === 'paths' ? paths : coverage).push(glob);
	}

	return { paths, coverage: coverage.length === 0 ? paths : coverage };
}

/** Each target in `root`'s fuzz/fuzz_targets/, by name. */
export function readTargets(root: string): Map<string, Target> {
	const files = readdirSync(join(root, TARGETS))
		.filter((name) => name.endsWith('.rs'))
		.sort();

	return new Map(
		files.map((name) => [name.slice(0, -'.rs'.length), headerOf(readFileSync(join(root, TARGETS, name), 'utf8'))])
	);
}

/** The first of `files` that `globs` select, less what every target leaves out. */
const reached = (globs: string[], files: string[]) => selected([...globs, ...EXCLUDED], files);

/** The targets that one of `changed` reaches, or all of them when one changes what every target is built or run with. */
export function select(targets: Map<string, Target>, changed: string[]): string[] {
	if (selected(EVERY_TARGET, changed) !== undefined) return [...targets.keys()];

	return [...targets].flatMap(([name, { paths }]) => (reached(paths, changed) === undefined ? [] : [name]));
}

/**
 * What's wrong with the targets' headers, against the repository's `files`: a target with no paths, a
 * glob that matches no file, or a measured file that a change to wouldn't run the target for.
 */
export function problems(targets: Map<string, Target>, files: string[]): string[] {
	return [...targets].flatMap(([name, { paths, coverage }]) => {
		const target = `${TARGETS}/${name}.rs`;

		if (paths.length === 0) return [`${target}: start it with \`// paths: <glob>\` lines for what it fuzzes`];

		const unmatched = [...new Set([...paths, ...coverage])].flatMap((glob) =>
			reached([glob], files) === undefined ? [`${target}: \`${glob}\` matches no file`] : []
		);

		const measured = selecting([...coverage, ...EXCLUDED]);
		const watched = selecting([...paths, ...EXCLUDED]);

		const unwatched = files.flatMap((file) =>
			measured(file) && !watched(file)
				? [`${target}: \`${file}\` is measured, so its \`// paths:\` have to name it too`]
				: []
		);

		return [...unmatched, ...unwatched];
	});
}

/**
 * The folders `target`'s alias in .cargo/config.toml hands libFuzzer, the corpus and then the seeds,
 * so a coverage replay reads what fuzzing does. Every alias there is one string (cargo.rego).
 */
export function corpora(config: string, target: string): string[] {
	const alias = new RegExp(`^fuzz-${target} = "fuzz run ${target} ([^"]*?) --(?: |")`, 'm').exec(config)?.[1];

	if (alias === undefined) throw new Error(`.cargo/config.toml has no fuzz-${target} alias`);

	return alias.split(' ');
}

const changedFiles = (root: string, base: string) =>
	execFileSync('git', ['diff', '--name-only', base, 'HEAD'], { cwd: root, encoding: 'utf8' })
		.split('\n')
		.filter(Boolean);

/** Runs a command from `root` and gives the lines to print and whether it passed. */
export function run([command, argument, ...extra]: string[], root = '.'): Outcome {
	if (command === 'select' && extra.length === 0) {
		const targets = readTargets(root);
		const chosen = argument === undefined ? [...targets.keys()] : select(targets, changedFiles(root, argument));

		return { lines: [chosen.join(' ')], ok: true };
	}

	if (command === 'corpora' && argument !== undefined && extra.length === 0) {
		return { lines: corpora(readFileSync(join(root, '.cargo/config.toml'), 'utf8'), argument), ok: true };
	}

	return { lines: ['usage: cli.ts select [<base>] | corpora <target>'], ok: false };
}
