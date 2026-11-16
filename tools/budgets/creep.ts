// How much main's budgets grew over a stretch of commits, and whose pull requests grew them. Each
// pull request is held to its own growth, so many small ones under their allowances can still add
// up: this reports the total, split into what `Budget-Raise` trailers declared and what came in
// under allowances, and fails once the undeclared part passes CAP allowances.
//
//   node tools/budgets/cli.ts creep <job> <points-dir>
//
// <points-dir> holds a folder per measured commit on main's first-parent line, oldest first, each
// named `<order>-<sha>` and holding that commit's `<area>.json` measurements, as the weekly run
// lays them out from the CI runs' artifacts.
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { type Budget, type Measured, type Outcome, type Value, areasOf, declared, grownBy } from './budgets.ts';

/** How many allowances of undeclared growth a metric may gather over the stretch. */
export const CAP = 5;

/** A measured commit on main, and what came in since the one before it. */
export type Point = { sha: string; measured: Record<string, Measured>; prs: number[]; raised: Map<string, number> };

/** One metric over the stretch. */
export type Creep = {
	metric: string;
	unit: string;
	growth: number;
	declared: number;
	undeclared: number;
	allowance: number;
	/** Pull requests by how much they grew it, largest first. */
	shares: [pr: string, growth: number][];
};

/** Each metric of `areas` over `points`: growth between consecutive points, declared up to its raises. */
export function creep(points: Point[], areas: [string, Budget][]): Creep[] {
	return areas.flatMap(([area, budget]) =>
		Object.entries(budget.metrics).map(([name, { allowance, unit }]) => {
			const totals: Creep = {
				metric: `${area}/${name}`,
				unit,
				growth: 0,
				declared: 0,
				undeclared: 0,
				allowance,
				shares: []
			};

			// A commit that didn't measure the metric passes its pull requests and raises on to the next one that did.
			let base: Value | undefined;
			const prs: number[] = [];
			let raised = 0;

			for (const point of points) {
				const head = point.measured[area]?.[name];

				prs.push(...point.prs);
				raised += point.raised.get(totals.metric) ?? 0;

				if (head === undefined) continue;

				const growth = base === undefined ? 0 : grownBy(base, head);
				const covered = Math.max(0, Math.min(growth, raised));
				const by = prs.length > 0 ? prs.map((pr) => `#${pr}`).join(', ') : point.sha.slice(0, 9);

				totals.growth += growth;
				totals.declared += covered;
				totals.undeclared += growth - covered;

				if (growth > 0) totals.shares.push([by, growth]);

				base = head;
				prs.length = 0;
				raised = 0;
			}

			return { ...totals, shares: totals.shares.toSorted(([, a], [, b]) => b - a) };
		})
	);
}

/** The metrics whose undeclared growth passed CAP allowances. */
export function crept(rows: Creep[]): string[] {
	return rows.flatMap(({ metric, unit, undeclared, allowance }) =>
		undeclared > 0 && undeclared > CAP * allowance
			? [
					`${metric}: ${undeclared} ${unit} of growth no Budget-Raise declared, over ${CAP} allowances (${CAP * allowance})`
				]
			: []
	);
}

/** `rows` as a Markdown table, with the three pull requests that grew each metric most. */
export function report(rows: Creep[]): string[] {
	const lines = rows.map(({ metric, unit, growth, declared: raised, undeclared, shares }) => {
		const top = shares
			.slice(0, 3)
			.map(([pr, by]) => `${pr} +${by}`)
			.join(', ');

		return `| ${metric} | ${growth} ${unit} | ${raised} | ${undeclared} | ${top} |`;
	});

	return ['| Metric | Growth | Declared | Undeclared | Most from |', '| --- | --- | --- | --- | --- |', ...lines];
}

const git = (...args: string[]) => execFileSync('git', args, { encoding: 'utf8' });

/** The points in `dir`, each with the pull requests merged and the raises declared since the one before. */
function pointsIn(dir: string): Point[] {
	const folders = readdirSync(dir).toSorted();

	return folders.map((folder, index) => {
		const sha = folder.slice(folder.indexOf('-') + 1);
		const files = readdirSync(join(dir, folder)).filter((file) => file.endsWith('.json'));

		// SAFETY: the folders hold what the CI jobs measured, `{ "<metric>": <number or names> }`.
		const measured = Object.fromEntries(
			files.map((file) => [
				file.replace(/\.json$/, ''),
				JSON.parse(readFileSync(join(dir, folder, file), 'utf8')) as Measured
			])
		);

		const previous = folders[index - 1];

		if (previous === undefined) return { sha, measured, prs: [], raised: new Map() };

		const range = `${previous.slice(previous.indexOf('-') + 1)}..${sha}`;
		const subjects = git('log', '--first-parent', '--merges', '--format=%s', range).split('\n');
		const prs = subjects.flatMap((subject) => /^Merge pull request #(\d+) /.exec(subject)?.[1] ?? []).map(Number);

		// Trailers only, as a date past the body lines' end: a body line says no amount.
		const raised = declared(git('log', '--format=%(trailers:key=Budget-Raise)', range), '9999-12-31');

		return { sha, measured, prs, raised };
	});
}

/** Runs `creep <job> <points-dir>`. */
export function run([command, ...args]: string[]): Outcome {
	if (command !== 'creep' || args.length !== 2) return { lines: ['usage: cli.ts creep <job> <points-dir>'], ok: false };

	// SAFETY: the length is checked just above.
	const [job, dir] = args as [string, string];

	const rows = existsSync(dir) ? creep(pointsIn(dir), areasOf(job)) : [];
	const found = crept(rows);

	return { lines: [...report(rows), '', ...found], ok: found.length === 0 };
}
