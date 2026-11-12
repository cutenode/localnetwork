// Per-crate coverage floors for the Rust workspace, beside the workspace-wide thresholds in
// .cargo/config.toml: a small crate can lose most of its coverage while the workspace's total holds.
//
//   node tools/coverage/cli.ts check <coverage.json>    fails on a crate under its floor
//   node tools/coverage/cli.ts ratchet <coverage.json>  raises each floor to just under the run's
//
// <coverage.json> is cargo-llvm-cov's JSON summary (`cargo coverage --json --summary-only`), which
// locallibrary-ci.yml's coverage job writes. A crate is the folder of its manifest: apps/<name>,
// crates/<name>, plugins/<name> or vendor/<name>. Floors are in tools/coverage/floors.json.
import { readFileSync, writeFileSync } from 'node:fs';

export const METRICS = ['lines', 'functions', 'regions'] as const;

export type Metric = (typeof METRICS)[number];

export type Percents = Record<Metric, number>;

type Count = { count: number; covered: number };

/** The part of cargo-llvm-cov's JSON summary this reads. */
export type Report = { data: { files: { filename: string; summary: Record<Metric, Count> }[] }[] };

export type Floors = { $schema: string; floors: Record<string, Percents> };

const FLOORS = 'tools/coverage/floors.json';

// How far under a run's percentage `ratchet` sets a floor: llvm-cov's counts move a little between
// runs of the same code (a child process killed while it writes its profile).
const MARGIN = 0.2;

/** The crate a file belongs to, from its path: `<area>/<name>` for the areas the workspace has. */
export function crateOf(filename: string): string | undefined {
	return /(?:^|\/)((?:apps|crates|plugins|vendor)\/[^/]+)\//.exec(filename)?.[1];
}

/** Each crate's percentage of lines, functions and regions covered. */
export function byCrate(report: Report): Map<string, Percents> {
	const totals = new Map<string, Record<Metric, Count>>();

	for (const { filename, summary } of report.data.flatMap((export_) => export_.files)) {
		const crate = crateOf(filename);

		if (crate === undefined) continue;

		const total = totals.get(crate) ?? {
			lines: { count: 0, covered: 0 },
			functions: { count: 0, covered: 0 },
			regions: { count: 0, covered: 0 }
		};

		for (const metric of METRICS) {
			total[metric].count += summary[metric].count;
			total[metric].covered += summary[metric].covered;
		}

		totals.set(crate, total);
	}

	const percent = ({ count, covered }: Count) => (count === 0 ? 100 : (100 * covered) / count);

	return new Map(
		[...totals].map(([crate, total]) => [
			crate,
			{ lines: percent(total.lines), functions: percent(total.functions), regions: percent(total.regions) }
		])
	);
}

/** What's wrong with `measured` against `floors`: a crate under one, or one with no floor. */
export function problems(measured: Map<string, Percents>, floors: Floors['floors']): string[] {
	const found: string[] = [];

	for (const [crate, percents] of measured) {
		const floor = floors[crate];

		if (floor === undefined) {
			found.push(`${crate}: no floor; run the ratchet and commit tools/coverage/floors.json`);
			continue;
		}

		for (const metric of METRICS) {
			if (percents[metric] < floor[metric]) {
				found.push(`${crate}: ${metric} ${percents[metric].toFixed(2)}% is under its floor of ${floor[metric]}%`);
			}
		}
	}

	for (const crate of Object.keys(floors)) {
		if (!measured.has(crate)) found.push(`${crate}: has a floor but no coverage; remove it from ${FLOORS}`);
	}

	return found;
}

/** `floors` with each one raised to just under what was measured, and one for each new crate: never lowered. */
export function ratcheted(measured: Map<string, Percents>, floors: Floors['floors']): Floors['floors'] {
	const under = (value: number) => Math.max(0, Math.floor((value - MARGIN) * 10) / 10);

	const entries = [...measured].map(([crate, percents]): [string, Percents] => {
		const floor = floors[crate];
		const raised = (metric: Metric) => Math.max(floor?.[metric] ?? 0, under(percents[metric]));

		return [crate, { lines: raised('lines'), functions: raised('functions'), regions: raised('regions') }];
	});

	return Object.fromEntries(entries.sort(([a], [b]) => a.localeCompare(b)));
}

// SAFETY: the report is cargo-llvm-cov's own JSON export, and floors.json follows floors.schema.json
// (tools/schemas/schemas.test.ts checks it).
const readJson = <T>(path: string) => JSON.parse(readFileSync(path, 'utf8')) as T;

/** What a command prints, and whether it passed. */
export type Outcome = { lines: string[]; ok: boolean };

/** Runs a command and gives the lines to print and whether it passed. */
export function run([command, report, ...extra]: string[]): Outcome {
	if (report === undefined || extra.length > 0 || (command !== 'check' && command !== 'ratchet')) {
		return { lines: ['usage: cli.ts check|ratchet <coverage.json>'], ok: false };
	}

	const measured = byCrate(readJson<Report>(report));
	const floors = readJson<Floors>(FLOORS);

	if (command === 'ratchet') {
		writeFileSync(FLOORS, `${JSON.stringify({ ...floors, floors: ratcheted(measured, floors.floors) }, null, '\t')}\n`);

		return { lines: [`wrote ${FLOORS}`], ok: true };
	}

	const found = problems(measured, floors.floors);

	return { lines: found, ok: found.length === 0 };
}
