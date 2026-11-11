// The performance budgets in benchmarks/budgets/, one file per area, and the ratchet that holds
// them. Each number is one a run gives exactly (bytes, allocations, instructions), never a time.
//
//   node tools/budgets/cli.ts check <measured-dir> <job>    fails over or well under a limit
//   node tools/budgets/cli.ts ratchet <measured-dir> <job>  lowers limits to what was measured
//   node tools/budgets/cli.ts raises <base-ref> <body-file> fails on an undeclared raise
//
// <measured-dir>/<area>.json holds `{ "<metric>": <number> }`, and each area's file says which job
// measures it, so a new area adds a file and nothing else. See benchmarks/README.md.
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

export type Metric = { limit: number | null; unit: string; measures: string };

export type Budget = { $schema: string; job: string; tolerance_percent: number; metrics: Record<string, Metric> };

export type Measured = Record<string, number>;

const BUDGETS = 'benchmarks/budgets';

const budgetPath = (area: string) => join(BUDGETS, `${area}.json`);

// SAFETY: the budget files follow benchmarks/budget.schema.json (budgets.test.ts checks them), and
// the measured ones are what the CI jobs write, `{ "<metric>": <number> }`.
const readJson = <T>(path: string) => JSON.parse(readFileSync(path, 'utf8')) as T;

const written = (budget: Budget) => `${JSON.stringify(budget, null, '\t')}\n`;

/** What's wrong with `measured` against `budget`: nothing passes for a metric with no limit yet. */
export function problems(area: string, budget: Budget, measured: Measured): string[] {
	const found: string[] = [];
	const tolerance = budget.tolerance_percent / 100;

	for (const [name, { limit, unit }] of Object.entries(budget.metrics)) {
		const value = measured[name];

		if (value === undefined) {
			found.push(`${area}/${name}: not measured`);
		} else if (limit !== null && value > limit * (1 + tolerance)) {
			found.push(`${area}/${name}: ${value} ${unit} is over its limit of ${limit}`);
		} else if (limit !== null && value < limit * (1 - tolerance)) {
			found.push(`${area}/${name}: ${value} ${unit} beats its limit of ${limit}: run the ratchet and commit it`);
		}
	}

	for (const name of Object.keys(measured)) {
		if (!(name in budget.metrics)) found.push(`${area}/${name}: measured, but has no budget`);
	}

	return found;
}

/** `budget` with each limit lowered to what was measured, or set if it had none: never raised. */
export function ratcheted(budget: Budget, measured: Measured): Budget {
	const metrics = Object.fromEntries(
		Object.entries(budget.metrics).map(([name, metric]) => {
			const value = measured[name];
			const lower = value !== undefined && (metric.limit === null || value < metric.limit);

			return [name, lower ? { ...metric, limit: value } : metric];
		})
	);

	return { ...budget, metrics };
}

/** What makes `after` looser than `before`, each as the metric it's about (`<area>/<name>`). */
export function loosened(area: string, before: Budget, after: Budget | undefined): string[] {
	if (after === undefined) return Object.keys(before.metrics).map((name) => `${area}/${name}`);

	const raised = Object.entries(before.metrics).filter(([name, { limit }]) => {
		const now = after.metrics[name]?.limit;

		return limit !== null && (now === undefined || now === null || now > limit);
	});

	const names = raised.map(([name]) => `${area}/${name}`);

	return after.tolerance_percent > before.tolerance_percent ? [...names, `${area}/*`] : names;
}

/** The entries of `raised` that `body`, a pull request's, has no `budget-raise: <metric>: <why>` for. */
export function undeclared(raised: string[], body: string): string[] {
	const lines = body.split(/\r?\n/).map((line) => line.trim());

	return raised.filter((metric) => !lines.some((line) => line.startsWith(`budget-raise: ${metric}: `)));
}

function areasAt(ref: string): string[] {
	const listed = execFileSync('git', ['ls-tree', '--name-only', ref, `${BUDGETS}/`], { encoding: 'utf8' });

	return listed.split('\n').filter((path) => path.endsWith('.json'));
}

/** The areas `job` measures, each with its budget. */
function areasOf(job: string): [string, Budget][] {
	const areas = readdirSync(BUDGETS).map((file) => file.replace(/\.json$/, ''));
	const budgets = areas.map((area): [string, Budget] => [area, readJson<Budget>(budgetPath(area))]);

	return budgets.filter(([, budget]) => budget.job === job);
}

function check(dir: string, job: string): string[] {
	return areasOf(job).flatMap(([area, budget]) => {
		const measuredPath = join(dir, `${area}.json`);

		if (!existsSync(measuredPath)) return [`${area}: nothing measured, no ${measuredPath}`];

		return problems(area, budget, readJson<Measured>(measuredPath));
	});
}

function ratchet(dir: string, job: string): string[] {
	const changed: string[] = [];

	for (const [area, budget] of areasOf(job)) {
		const measuredPath = join(dir, `${area}.json`);

		if (!existsSync(measuredPath)) continue;
		const after = written(ratcheted(budget, readJson<Measured>(measuredPath)));

		if (after === written(budget)) continue;
		writeFileSync(budgetPath(area), after);
		changed.push(budgetPath(area));
	}

	return changed;
}

function raises(base: string, bodyFile: string): string[] {
	const here = new Set(readdirSync(BUDGETS));

	const raised = areasAt(base).flatMap((path) => {
		const file = path.slice(BUDGETS.length + 1);
		// SAFETY: the base's budget files passed budgets.test.ts's schema check when they merged.
		const before = JSON.parse(execFileSync('git', ['show', `${base}:${path}`], { encoding: 'utf8' })) as Budget;
		const after = here.has(file) ? readJson<Budget>(join(BUDGETS, file)) : undefined;

		return loosened(file.replace(/\.json$/, ''), before, after);
	});

	return undeclared(raised, readFileSync(bodyFile, 'utf8')).map(
		(metric) => `${metric}: loosened: add a line \`budget-raise: ${metric}: <why>\` to the pull request's body`
	);
}

/** What a command prints, and whether it passed. */
export type Outcome = { lines: string[]; ok: boolean };

/** Runs a command and gives the lines to print and whether it passed. */
export function run([command, first, second, ...extra]: string[]): Outcome {
	if (first === undefined || second === undefined || extra.length > 0) return usage();

	if (command === 'ratchet') return { lines: ratchet(first, second).map((path) => `lowered ${path}`), ok: true };

	if (command !== 'check' && command !== 'raises') return usage();
	const found = command === 'check' ? check(first, second) : raises(first, second);

	return { lines: found, ok: found.length === 0 };
}

function usage(): Outcome {
	return { lines: ['usage: cli.ts check|ratchet <measured-dir> <job> | raises <base-ref> <body-file>'], ok: false };
}
