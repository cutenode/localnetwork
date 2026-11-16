// The performance budgets in benchmarks/budgets/, one file per area. Each number is one a run gives
// exactly (bytes, allocations, instructions, package names), never a time, and a pull request is held
// to how much it grows that number over the commit it merges into, not to a committed limit, so
// main's own growth never turns a pull request red and pull requests never edit these files.
//
//   node tools/budgets/cli.ts check <job> <base-dir> <head-dir> <raises-file> [entries]
//       fails on a metric that grew past its allowance (times `entries`, the pull requests a merge
//       queue group holds) and no `budget-raise:` line in <raises-file> covers
//   node tools/budgets/cli.ts baseline <sha> <filter> <artifact> <dir>
//       see baseline.ts
//   node tools/budgets/cli.ts raises <base-ref> <body-file>
//       fails on an allowance widened or a metric dropped without a `budget-raise:` line
//
// <dir>/<area>.json holds `{ "<metric>": <number or list of names> }`, and each area's file says which
// job measures it, so a new area adds a file and nothing else. See benchmarks/README.md.
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

export type Metric = { allowance: number; unit: string; measures: string };

export type Budget = { $schema: string; job: string; metrics: Record<string, Metric> };

/** A count, or a list of names whose growth is the names that are new. */
export type Value = number | string[];

export type Measured = Record<string, Value>;

/** One metric of a check: what the base and head measured, and how much it grew. */
export type Row = {
	metric: string;
	unit: string;
	base?: Value;
	head?: Value;
	growth?: number;
	/** The names a list gained over its base. */
	added: string[];
	allowance: number;
};

const BUDGETS = 'benchmarks/budgets';

// SAFETY: the budget files follow benchmarks/budget.schema.json (tools/schemas/schemas.test.ts checks them), and
// the measured ones are what the CI jobs write, `{ "<metric>": <number or names> }`.
const readJson = <T>(path: string) => JSON.parse(readFileSync(path, 'utf8')) as T;

const newNames = (base: Value, head: Value) =>
	Array.isArray(head) && Array.isArray(base) ? head.filter((name) => !base.includes(name)) : [];

/** How much `head` grew over `base`: the difference of two counts, or how many names are new. */
function grownBy(base: Value, head: Value): number {
	if (!Array.isArray(head) && !Array.isArray(base)) return head - base;

	return newNames(base, head).length;
}

/** Each metric of `budget`, compared: a metric missing from `base` (a new one, or no base at all) has no growth. */
export function compared(area: string, budget: Budget, base: Measured | undefined, head: Measured): Row[] {
	return Object.entries(budget.metrics).map(([name, { allowance, unit }]) => {
		const metric = `${area}/${name}`;
		const [before, after] = [base?.[name], head[name]];
		const row: Row = { metric, unit, base: before, head: after, added: [], allowance };

		if (before === undefined || after === undefined) return row;

		return { ...row, growth: grownBy(before, after), added: newNames(before, after) };
	});
}

/** What fails in `rows`: growth past each allowance times `entries`, unless `raised` names the metric. */
export function problems(rows: Row[], entries: number, raised: string[]): string[] {
	return rows.flatMap(({ metric, unit, head, growth, added, allowance }) => {
		if (head === undefined) return [`${metric}: not measured`];

		const room = allowance * entries;

		if (growth === undefined || growth <= room || raised.includes(metric)) return [];
		const what = added.length > 0 ? `${added.join(', ')} added` : `${growth} ${unit} more than its base`;

		return [`${metric}: ${what}, over its allowance of ${room}: add \`budget-raise: ${metric}: <why>\` if it's meant`];
	});
}

/** The metrics a pull request's body declares a raise for, one `budget-raise: <metric>: <why>` line each. */
export function declared(body: string): string[] {
	const lines = body.split(/\r?\n/).map((line) => line.trim());

	return lines.flatMap((line) => /^budget-raise: (\S+): \S/.exec(line)?.[1] ?? []);
}

const shown = (value: Value | undefined) => {
	if (value === undefined) return '';

	return Array.isArray(value) ? `${value.length} names` : String(value);
};

/** `rows` as a Markdown table for the run's summary. */
export function table(rows: Row[]): string[] {
	const lines = rows.map(({ metric, unit, base, head, growth, allowance }) => {
		const grew = growth === undefined ? 'no base' : `${growth > 0 ? '+' : ''}${growth}`;

		return `| ${metric} | ${shown(base)} | ${shown(head)} | ${grew} | ${allowance} ${unit} |`;
	});

	return ['| Metric | Base | Head | Growth | Allowance |', '| --- | --- | --- | --- | --- |', ...lines];
}

/** What makes `after` looser than `before`, each as the metric it's about (`<area>/<name>`). */
export function loosened(area: string, before: Budget, after: Budget | undefined): string[] {
	const widened = Object.entries(before.metrics).filter(([name, { allowance }]) => {
		const now = after?.metrics[name]?.allowance;

		return now === undefined || now > allowance;
	});

	return widened.map(([name]) => `${area}/${name}`);
}

/** The areas `job` measures, each with its budget. */
function areasOf(job: string): [string, Budget][] {
	const areas = readdirSync(BUDGETS).map((file) => file.replace(/\.json$/, ''));
	const budgets = areas.map((area): [string, Budget] => [area, readJson<Budget>(join(BUDGETS, `${area}.json`))]);

	return budgets.filter(([, budget]) => budget.job === job);
}

function measuredIn(dir: string, area: string): Measured | undefined {
	const path = join(dir, `${area}.json`);

	return existsSync(path) ? readJson<Measured>(path) : undefined;
}

function check(job: string, baseDir: string, headDir: string, raisesFile: string, entries: number): Outcome {
	const raised = declared(readFileSync(raisesFile, 'utf8'));
	const rows: Row[] = [];
	const found: string[] = [];

	for (const [area, budget] of areasOf(job)) {
		const head = measuredIn(headDir, area);

		if (head === undefined) {
			found.push(`${area}: nothing measured, no ${join(headDir, `${area}.json`)}`);
			continue;
		}

		const areaRows = compared(area, budget, measuredIn(baseDir, area), head);
		const extra = Object.keys(head).filter((name) => !(name in budget.metrics));

		rows.push(...areaRows);
		found.push(
			...problems(areaRows, entries, raised),
			...extra.map((name) => `${area}/${name}: measured, but has no budget`)
		);
	}

	return { lines: [...table(rows), '', ...found], ok: found.length === 0 };
}

function raises(base: string, bodyFile: string): Outcome {
	const listed = execFileSync('git', ['ls-tree', '--name-only', base, `${BUDGETS}/`], { encoding: 'utf8' });
	const here = new Set(readdirSync(BUDGETS));

	const widened = listed
		.split('\n')
		.filter((path) => path.endsWith('.json'))
		.flatMap((path) => {
			const file = path.slice(BUDGETS.length + 1);
			// SAFETY: the base's budget files passed the schema check in tools/schemas/schemas.test.ts when they merged.
			const before = JSON.parse(execFileSync('git', ['show', `${base}:${path}`], { encoding: 'utf8' })) as Budget;
			const after = here.has(file) ? readJson<Budget>(join(BUDGETS, file)) : undefined;

			return loosened(file.replace(/\.json$/, ''), before, after);
		});

	const covered = declared(readFileSync(bodyFile, 'utf8'));

	const found = widened
		.filter((metric) => !covered.includes(metric))
		.map((metric) => `${metric}: loosened: add a line \`budget-raise: ${metric}: <why>\` to the pull request's body`);

	return { lines: found, ok: found.length === 0 };
}

/** What a command prints, and whether it passed. */
export type Outcome = { lines: string[]; ok: boolean };

/** Runs a command and gives the lines to print and whether it passed. */
export function run([command, ...args]: string[]): Outcome {
	if (command === 'check' && (args.length === 4 || args.length === 5)) {
		// SAFETY: the length is checked just above.
		const [job, baseDir, headDir, raisesFile, entries] = args as [string, string, string, string, string?];
		const groupSize = Number(entries ?? '1');

		if (Number.isInteger(groupSize) && groupSize > 0) return check(job, baseDir, headDir, raisesFile, groupSize);
	}

	if (command === 'raises' && args.length === 2) {
		// SAFETY: the length is checked just above.
		const [base, bodyFile] = args as [string, string];

		return raises(base, bodyFile);
	}

	return {
		lines: [
			'usage: cli.ts check <job> <base-dir> <head-dir> <raises-file> [entries]',
			'       cli.ts raises <base-ref> <body-file>'
		],
		ok: false
	};
}
