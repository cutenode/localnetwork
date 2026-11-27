// Each fuzz target's coverage from replaying its corpus (`cargo fuzz coverage`), as a share of the
// code its header names (`// coverage:`, or else `// paths:`, see targets.ts): the rest of what the
// target links, the standard library and the crates it's built on, says nothing about what it fuzzes,
// nor does the rest of a file it runs one function of. Against main's last
// weekly numbers, a drop is a warning and never a failure: a target's corpus is what a week of
// fuzzing kept, so a smaller one shows the fuzzing reaching less, not a bug.
//
//   node tools/fuzz/cli.ts report <exports> <last.json> <out>
//       reads <exports>/<target>.json, each `llvm-cov export -summary-only` of a target's replay, and
//       <last.json> when there is one; writes <out>/coverage.json (the numbers the next run compares
//       with), <out>/coverage.md (the table, and the drops under it) and <out>/drops.md (a line for
//       each target that fell, empty when none did), and prints a warning for each drop
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { selecting } from '../budgets/baseline.ts';
import { METRICS, type Metric, type Outcome, type Percents, type Report } from '../coverage/coverage.ts';
import { EXCLUDED, readTargets } from './targets.ts';

/** Each target's percentages, as coverage.json keeps them. */
export type Numbers = Record<string, Percents>;

// How far a percentage has to fall to warn: a replay of the same corpus over the same code can
// still move by a region or two (onelibrary encrypts each input under a salt drawn for the process).
const DROP = 0.1;

/** The share of the files under `root` that `globs` select, out of a target's export. */
export function measure(exported: Report, globs: string[], root: string): Percents {
	const counts: Record<Metric, { count: number; covered: number }> = {
		lines: { count: 0, covered: 0 },
		functions: { count: 0, covered: 0 },
		regions: { count: 0, covered: 0 }
	};

	const measured = selecting([...globs, ...EXCLUDED]);

	for (const { filename, summary } of exported.data.flatMap((export_) => export_.files)) {
		if (!measured(relative(root, filename))) continue;

		for (const metric of METRICS) {
			counts[metric].count += summary[metric].count;
			counts[metric].covered += summary[metric].covered;
		}
	}

	// A target none of whose files were built has nothing to show, which 0% makes plain.
	const percent = (metric: Metric) => {
		const { count, covered } = counts[metric];

		return count === 0 ? 0 : Math.round((10000 * covered) / count) / 100;
	};

	return { lines: percent('lines'), functions: percent('functions'), regions: percent('regions') };
}

/** How many points `now` is from `before`, to the hundredth both are kept to. */
const points = (now: number, before: number) => Math.round((now - before) * 100) / 100;

/** Each target of `now` that fell on some metric since `last`, as a line saying how far. */
export function drops(now: Numbers, last: Numbers): string[] {
	return Object.entries(now).flatMap(([target, percents]) => {
		const before = last[target];

		if (before === undefined) return [];

		const fell = METRICS.filter((metric) => points(percents[metric], before[metric]) <= -DROP);

		if (fell.length === 0) return [];

		return [`${target}: ${fell.map((metric) => `${metric} ${percents[metric]}% (was ${before[metric]}%)`).join(', ')}`];
	});
}

const change = (now: number, before: number | undefined) => {
	if (before === undefined) return '';

	const moved = points(now, before);

	return moved === 0 ? ' (same)' : ` (${moved > 0 ? '+' : ''}${moved})`;
};

/** The Markdown table of `now`, each percentage with its change since `last`. */
export function table(now: Numbers, last: Numbers | undefined): string[] {
	const rows = Object.entries(now).map(([target, percents]) => {
		const cells = METRICS.map((metric) => `${percents[metric]}%${change(percents[metric], last?.[target]?.[metric])}`);

		return `| ${target} | ${cells.join(' | ')} |`;
	});

	return [
		'## Fuzz coverage',
		'',
		"Each target's corpus replayed, as a share of the code its header names (`// coverage:`, or else `// paths:`)" +
			(last === undefined
				? '. No weekly numbers from main to compare with.'
				: ", in points since main's last weekly run."),
		'',
		'| Target | Lines | Functions | Regions |',
		'| --- | ---: | ---: | ---: |',
		...rows
	];
}

// SAFETY: the exports are llvm-cov's own JSON, and the numbers are what an earlier run of this wrote.
const readJson = <T>(path: string) => JSON.parse(readFileSync(path, 'utf8')) as T;

/** Measures every export in `exports` over what its target's header in `root` names, and writes `out`'s files. */
export function report(exports: string, lastPath: string, out: string, root: string): Outcome {
	const targets = readTargets(root);
	const last = existsSync(lastPath) ? readJson<Numbers>(lastPath) : undefined;

	const measured = readdirSync(exports)
		.filter((name) => name.endsWith('.json'))
		.sort()
		.map((name): [string, Percents] => {
			const target = name.slice(0, -'.json'.length);
			const header = targets.get(target);

			if (header === undefined) throw new Error(`${name}: no target ${target} in fuzz/fuzz_targets/`);

			return [target, measure(readJson<Report>(join(exports, name)), header.coverage, resolve(root))];
		});

	const now = Object.fromEntries(measured);
	const fell = last === undefined ? [] : drops(now, last);

	const listed = fell.map((line) => `- ${line}`);
	const warned = listed.length === 0 ? [] : ['', "Dropped since main's last weekly run:", '', ...listed];

	mkdirSync(out, { recursive: true });
	writeFileSync(join(out, 'coverage.json'), `${JSON.stringify(now, null, '\t')}\n`);
	writeFileSync(join(out, 'coverage.md'), `${[...table(now, last), ...warned].join('\n')}\n`);
	writeFileSync(join(out, 'drops.md'), listed.map((line) => `${line}\n`).join(''));

	return { lines: fell.map((line) => `::warning title=Fuzz coverage dropped::${line}`), ok: true };
}

/** Runs a command from `root` and gives the lines to print and whether it passed. */
export function run([command, exports, last, out, ...extra]: string[], root = '.'): Outcome {
	if (command !== 'report' || out === undefined || extra.length > 0) {
		return { lines: ['usage: cli.ts report <exports> <last.json> <out>'], ok: false };
	}

	return report(exports, last, out, root);
}
