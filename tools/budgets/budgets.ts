// The performance budgets in benchmarks/budgets/, one file per area. Each number is one a run gives
// exactly (bytes, allocations, instructions, package names), never a time, and a pull request is held
// to how much it grows that number over the commit it merges into, not to a committed limit, so
// main's own growth never turns a pull request red and pull requests never edit these files.
//
//   node tools/budgets/cli.ts check <job> <base-dir> <head-dir> [<raises-file> [entries]]
//       fails on a metric that grew past its allowance (times `entries`, the pull requests a merge
//       queue group holds) and the raises <raises-file> declares for it; with no <raises-file>, none
//   node tools/budgets/cli.ts baseline <sha> <filter> <artifact> <dir>
//       see baseline.ts
//   node tools/budgets/cli.ts noise <job> <dir> <dir>...
//       fails on a metric whose measurements of one commit, one per <dir>, spread over half its
//       allowance, which then can't tell a change from noise
//   node tools/budgets/cli.ts creep <job> <points-dir>
//       see creep.ts
//   node tools/budgets/cli.ts raises <base-ref> <raises-file>
//       fails on an allowance widened or a metric dropped that no raise names
//   node tools/budgets/cli.ts current <base-ref> <branch> <repo> <pull-request>
//       fails when <base-ref> predates <branch>'s last change to tools/budgets/, and says how to get
//       a current merge commit
//
// A raise is a commit's `Budget-Raise: <area>/<metric> +<amount>: <why>` trailer, which <raises-file>
// holds as `git log --format='%(trailers:key=Budget-Raise)'` prints it: in a commit, review covers it
// and the merge queue can read it. The pull request body's `budget-raise: <metric>: <why>` line, which
// allows any growth, is still read until 2026-11-30, for pull requests opened before trailers.
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
export function grownBy(base: Value, head: Value): number {
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

/** What fails in `rows`: growth past each allowance times `entries`, plus what `raised` declares for the metric. */
export function problems(rows: Row[], entries: number, raised: Map<string, number>): string[] {
	return rows.flatMap(({ metric, unit, head, growth, added, allowance }) => {
		if (head === undefined) return [`${metric}: not measured`];

		const room = allowance * entries + (raised.get(metric) ?? 0);

		if (growth === undefined || growth <= room) return [];
		const what = added.length > 0 ? `${added.join(', ')} added` : `${growth} ${unit} more than its base`;
		const trailer = `Budget-Raise: ${metric} +${growth - room}: <why>`;

		return [
			`${metric}: ${what}, ${growth - room} over what's allowed: if it's meant, add a commit with the trailer \`${trailer}\``
		];
	});
}

// Until this date, a pull request body's `budget-raise:` line still allows any growth.
const BODY_LINES_UNTIL = '2026-11-30';

/**
 * How much each metric may grow on top of its allowance: the sum of its `Budget-Raise:` trailers in
 * `text`, or any amount for a `budget-raise:` body line until BODY_LINES_UNTIL.
 */
export function declared(text: string, today = new Date().toISOString().slice(0, 10)): Map<string, number> {
	const raised = new Map<string, number>();
	const lines = text.split(/\r?\n/).map((line) => line.trim());

	for (const line of lines) {
		// git prints a trailer's key as the commit spelled it.
		const trailer = /^budget-raise: (\S+) \+(\d+): \S/i.exec(line);
		const body = today <= BODY_LINES_UNTIL ? /^budget-raise: (\S+): \S/.exec(line) : null;

		if (trailer?.[1] !== undefined) raised.set(trailer[1], (raised.get(trailer[1]) ?? 0) + Number(trailer[2]));

		if (body?.[1] !== undefined) raised.set(body[1], Infinity);
	}

	return raised;
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

/** How far apart repeated measurements of one commit are: the range of a count, or how many names aren't in every run. */
export function spread(values: Value[]): number {
	const counts = values.flatMap((value) => (Array.isArray(value) ? [] : [value]));
	const lists = values.flatMap((value) => (Array.isArray(value) ? [value] : []));

	if (lists.length > 0) {
		const names = new Set(lists.flat());

		return [...names].filter((name) => !lists.every((list) => list.includes(name))).length;
	}

	return counts.length === 0 ? 0 : Math.max(...counts) - Math.min(...counts);
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
export function areasOf(job: string): [string, Budget][] {
	const areas = readdirSync(BUDGETS).map((file) => file.replace(/\.json$/, ''));
	const budgets = areas.map((area): [string, Budget] => [area, readJson<Budget>(join(BUDGETS, `${area}.json`))]);

	return budgets.filter(([, budget]) => budget.job === job);
}

function measuredIn(dir: string, area: string): Measured | undefined {
	const path = join(dir, `${area}.json`);

	return existsSync(path) ? readJson<Measured>(path) : undefined;
}

function check(
	job: string,
	baseDir: string,
	headDir: string,
	raisesFile: string | undefined,
	entries: number
): Outcome {
	const raised = raisesFile === undefined ? new Map<string, number>() : declared(readFileSync(raisesFile, 'utf8'));
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

/** Fails a metric whose repeated measurements in `dirs` spread over half its allowance: the allowance is too tight. */
function noise(job: string, dirs: string[]): Outcome {
	const lines: string[] = ['| Metric | Spread | Allowance |', '| --- | --- | --- |'];
	const found: string[] = [];

	for (const [area, budget] of areasOf(job)) {
		const runs = dirs.flatMap((dir) => {
			const measured = measuredIn(dir, area);

			return measured === undefined ? [] : [measured];
		});

		for (const [name, { allowance, unit }] of Object.entries(budget.metrics)) {
			const width = spread(runs.flatMap((run) => (run[name] === undefined ? [] : [run[name]])));

			lines.push(`| ${area}/${name} | ${width} | ${allowance} ${unit} |`);

			if (width * 2 > allowance)
				found.push(
					`${area}/${name}: runs of one commit spread ${width} ${unit}, over half its allowance of ${allowance}`
				);
		}
	}

	return { lines: [...lines, '', ...found], ok: found.length === 0 };
}

function raises(base: string, raisesFile: string): Outcome {
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

	const covered = declared(readFileSync(raisesFile, 'utf8'));

	const found = widened
		.filter((metric) => !covered.has(metric))
		.map(
			(metric) =>
				`${metric}: loosened: if it's meant, add a commit with the trailer \`Budget-Raise: ${metric} +0: <why>\``
		);

	return { lines: found, ok: found.length === 0 };
}

/**
 * Whether `base` has `branch`'s budget tooling. A pull request's merge commit holds its base as it was
 * when GitHub made it, and a close and reopen can leave an old one, which would measure and check with
 * the old tooling (LOC-710).
 */
function current(base: string, branch: string, repo: string, number: string): Outcome {
	const changed = execFileSync('git', ['log', '-1', '--format=%H', branch, '--', 'tools/budgets'], {
		encoding: 'utf8'
	}).trim();

	try {
		execFileSync('git', ['merge-base', '--is-ancestor', changed, base]);

		return { lines: [], ok: true };
	} catch {
		const short = changed.slice(0, 9);

		// A close and reopen gets a new merge commit once GitHub has made one on the current base, which
		// keeps the pull request's history as it is; updating the branch is for when it doesn't.
		return {
			lines: [
				`${base} predates ${short}, ${branch}'s last change to tools/budgets/, so this would check with old tooling. Close and reopen the pull request once \`gh api repos/${repo}/pulls/${number} --jq .merge_commit_sha\` prints a merge commit whose first parent contains ${short}, and update the branch only if that doesn't happen.`
			],
			ok: false
		};
	}
}

/** What a command prints, and whether it passed. */
export type Outcome = { lines: string[]; ok: boolean };

/** Runs a command and gives the lines to print and whether it passed. */
export function run([command, ...args]: string[]): Outcome {
	if (command === 'check' && args.length >= 3 && args.length <= 5) {
		// SAFETY: the length is checked just above.
		const [job, baseDir, headDir, raisesFile, entries] = args as [string, string, string, string?, string?];
		const groupSize = Number(entries ?? '1');

		if (Number.isInteger(groupSize) && groupSize > 0) return check(job, baseDir, headDir, raisesFile, groupSize);
	}

	if (command === 'noise' && args.length >= 3) {
		// SAFETY: the length is checked just above.
		const [job, ...dirs] = args as [string, ...string[]];

		return noise(job, dirs);
	}

	if (command === 'raises' && args.length === 2) {
		// SAFETY: the length is checked just above.
		const [base, raisesFile] = args as [string, string];

		return raises(base, raisesFile);
	}

	if (command === 'current' && args.length === 4) {
		// SAFETY: the length is checked just above.
		const [base, branch, repo, number] = args as [string, string, string, string];

		return current(base, branch, repo, number);
	}

	return {
		lines: [
			'usage: cli.ts check <job> <base-dir> <head-dir> [<raises-file> [entries]]',
			'       cli.ts noise <job> <dir> <dir>...',
			'       cli.ts raises <base-ref> <raises-file>',
			'       cli.ts current <base-ref> <branch> <repo> <pull-request>'
		],
		ok: false
	};
}
