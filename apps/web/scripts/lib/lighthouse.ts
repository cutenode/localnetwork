// What scripts/lighthouse.ts keeps of each page's Lighthouse runs, and how scripts/lighthouse-history.ts
// holds that against the pages' history. No dependencies, so CI's history job runs it on plain Node.

export type FormFactor = 'mobile' | 'desktop';

export type Account = 'dj' | 'admin';

/** A number tracked over time: a category's score, or an audit's numericValue. */
export type Metric = {
	id: string;
	label: string;
	category?: string;
	audit?: string;
	unit: 'score' | 'ms' | 'bytes' | 'count';
	higherIsBetter: boolean;
	/** A change only counts past both: this much, and (when set) this share of the baseline. */
	tolerance: { absolute: number; relative?: number };
};

// Tolerances sit above the run-to-run noise of Lighthouse's simulated throttling on a median of three:
// the category scores other than performance don't move between runs, so any drop counts.
export const METRICS: Metric[] = [
	{
		id: 'performance',
		label: 'Performance',
		category: 'performance',
		unit: 'score',
		higherIsBetter: true,
		tolerance: { absolute: 0.05 }
	},
	{
		id: 'accessibility',
		label: 'Accessibility',
		category: 'accessibility',
		unit: 'score',
		higherIsBetter: true,
		tolerance: { absolute: 0.01 }
	},
	{
		id: 'best-practices',
		label: 'Best practices',
		category: 'best-practices',
		unit: 'score',
		higherIsBetter: true,
		tolerance: { absolute: 0.01 }
	},
	{ id: 'seo', label: 'SEO', category: 'seo', unit: 'score', higherIsBetter: true, tolerance: { absolute: 0.01 } },
	{
		id: 'fcp',
		label: 'FCP',
		audit: 'first-contentful-paint',
		unit: 'ms',
		higherIsBetter: false,
		tolerance: { absolute: 100, relative: 0.1 }
	},
	{
		id: 'lcp',
		label: 'LCP',
		audit: 'largest-contentful-paint',
		unit: 'ms',
		higherIsBetter: false,
		tolerance: { absolute: 150, relative: 0.1 }
	},
	{
		id: 'tbt',
		label: 'TBT',
		audit: 'total-blocking-time',
		unit: 'ms',
		higherIsBetter: false,
		tolerance: { absolute: 50, relative: 0.25 }
	},
	{
		id: 'cls',
		label: 'CLS',
		audit: 'cumulative-layout-shift',
		unit: 'count',
		higherIsBetter: false,
		tolerance: { absolute: 0.02 }
	},
	{
		id: 'si',
		label: 'Speed Index',
		audit: 'speed-index',
		unit: 'ms',
		higherIsBetter: false,
		tolerance: { absolute: 200, relative: 0.15 }
	},
	{
		id: 'ttfb',
		label: 'Server response',
		audit: 'server-response-time',
		unit: 'ms',
		higherIsBetter: false,
		tolerance: { absolute: 100, relative: 0.5 }
	},
	{
		id: 'js',
		label: 'JS execution',
		audit: 'bootup-time',
		unit: 'ms',
		higherIsBetter: false,
		tolerance: { absolute: 100, relative: 0.25 }
	},
	{
		id: 'bytes',
		label: 'Transfer size',
		audit: 'total-byte-weight',
		unit: 'bytes',
		higherIsBetter: false,
		tolerance: { absolute: 10_240, relative: 0.05 }
	},
	{
		id: 'dom',
		label: 'DOM elements',
		audit: 'dom-size',
		unit: 'count',
		higherIsBetter: false,
		tolerance: { absolute: 100, relative: 0.1 }
	}
];

/** The parts of a Lighthouse result (LHR) read here. */
export type Lhr = {
	lighthouseVersion: string;
	finalDisplayedUrl: string;
	runtimeError?: { code: string; message: string };
	environment: { benchmarkIndex: number };
	categories: Record<string, { score: number | null }>;
	audits: Record<string, { score: number | null; numericValue?: number }>;
};

export type Stat = { median: number; min: number; max: number };

export type PageSummary = {
	path: string;
	account?: Account;
	/** Runs that produced a result. */
	runs: number;
	/** Bad results: failed runs, a page that didn't load as itself, and lhci's failed assertions. */
	problems: string[];
	metrics: Record<string, Stat>;
};

/** One form factor's pages from a run of scripts/lighthouse.ts (lighthouse-full/summary-<form factor>.json). */
export type Summary = {
	formFactor: FormFactor;
	lighthouseVersion: string;
	benchmarkIndex: number;
	runs: number;
	pages: PageSummary[];
};

/** A line of the history file: each page's medians, keyed `<form factor> <path>`. */
export type HistoryEntry = {
	date: string;
	commit: string;
	lighthouseVersion: string;
	benchmarkIndex: number;
	results: Record<string, Record<string, number>>;
};

export const median = (values: number[]) => {
	const sorted = values.toSorted((a, b) => a - b);
	const middle = Math.floor(sorted.length / 2);

	return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};

const metricValue = (lhr: Lhr, metric: Metric) =>
	metric.category ? lhr.categories[metric.category]?.score : lhr.audits[metric.audit ?? '']?.numericValue;

/** One page's runs, with what went wrong in them. */
export function summarizePage(
	page: { path: string; account?: Account },
	lhrs: Lhr[],
	assertionFailures: string[] = []
): PageSummary {
	const problems: string[] = [];
	const ok = lhrs.filter((lhr) => !lhr.runtimeError);

	for (const lhr of lhrs.filter((lhr) => lhr.runtimeError)) {
		problems.push(`a run failed: ${lhr.runtimeError?.code} (${lhr.runtimeError?.message})`);
	}

	if (!lhrs.length) problems.push('no run produced a result');

	// A signed-in page measured signed out lands on /login, and a missing one on the error page.
	const landed = new Set(
		ok.map(({ finalDisplayedUrl }) => new URL(finalDisplayedUrl)).map((url) => url.pathname + url.search)
	);

	for (const path of landed) if (path !== page.path) problems.push(`ended up on ${path}`);

	if (ok.some((lhr) => lhr.audits['http-status-code']?.score === 0)) problems.push('answered with an error status');

	problems.push(...assertionFailures);

	const metrics: Record<string, Stat> = {};

	for (const metric of METRICS) {
		const values = ok.flatMap((lhr) => metricValue(lhr, metric) ?? []);

		if (values.length)
			metrics[metric.id] = { median: median(values), min: Math.min(...values), max: Math.max(...values) };
	}

	return { ...page, runs: ok.length, problems, metrics };
}

export type Change = { key: string; metric: Metric; baseline: number; current: number };

export type Comparison = {
	entry: HistoryEntry;
	/** Worse than the median of the last few runs by more than the metric's tolerance. */
	regressions: Change[];
	/** Not a regression against the last few runs, but worse than the runs before them: a slow creep. */
	drifts: Change[];
	improvements: Change[];
	problems: { key: string; problem: string }[];
	/** Runs of one page that spread wider than twice the metric's tolerance, so its median is shaky. */
	unstable: { key: string; metric: Metric; stat: Stat }[];
	/** Pages with no history yet. */
	added: string[];
	/** Why this run's numbers may not line up with its baseline's. */
	notes: string[];
	baselineRuns: number;
};

export const RECENT_RUNS = 5;

export const OLDER_RUNS = 20;

const limit = (metric: Metric, baseline: number) =>
	Math.max(metric.tolerance.absolute, (metric.tolerance.relative ?? 0) * Math.abs(baseline));

/** How much worse `current` is than `baseline`, positive when worse. */
const worsening = (metric: Metric, baseline: number, current: number) =>
	metric.higherIsBetter ? baseline - current : current - baseline;

const baselineOf = (entries: HistoryEntry[], key: string, metric: Metric) => {
	const values = entries.flatMap((entry) => entry.results[key]?.[metric.id] ?? []);

	return values.length ? median(values) : undefined;
};

/** This run's summaries against the history file's entries (oldest first), and the entry to record. */
export function compare(
	summaries: Summary[],
	history: HistoryEntry[],
	run: { date: string; commit: string }
): Comparison {
	const keyed = summaries.flatMap((summary) =>
		summary.pages.map((page) => ({ key: `${summary.formFactor} ${page.path}`, page }))
	);

	const entry: HistoryEntry = {
		...run,
		lighthouseVersion: summaries[0]?.lighthouseVersion ?? '',
		benchmarkIndex: median(summaries.map((summary) => summary.benchmarkIndex)),
		results: Object.fromEntries(
			keyed.map(({ key, page }) => [
				key,
				Object.fromEntries(Object.entries(page.metrics).map(([id, stat]) => [id, stat.median]))
			])
		)
	};

	const recent = history.slice(-RECENT_RUNS);
	const older = history.slice(-RECENT_RUNS - OLDER_RUNS, -RECENT_RUNS);

	const comparison: Comparison = {
		entry,
		regressions: [],
		drifts: [],
		improvements: [],
		problems: keyed.flatMap(({ key, page }) => page.problems.map((problem) => ({ key, problem }))),
		unstable: [],
		added: [],
		notes: [],
		baselineRuns: recent.length
	};

	for (const { key, page } of keyed) {
		if (!recent.some((past) => past.results[key])) comparison.added.push(key);

		for (const metric of METRICS) {
			const stat = page.metrics[metric.id];

			if (!stat) continue;

			if (stat.max - stat.min > 2 * limit(metric, stat.median)) comparison.unstable.push({ key, metric, stat });

			const baseline = baselineOf(recent, key, metric);

			if (baseline === undefined) continue;

			const change = { key, metric, baseline, current: stat.median };
			const worse = worsening(metric, baseline, stat.median);

			if (worse > limit(metric, baseline)) {
				comparison.regressions.push(change);
				continue;
			}

			if (-worse > limit(metric, baseline)) comparison.improvements.push(change);

			const longBaseline = baselineOf(older, key, metric);

			if (longBaseline !== undefined && worsening(metric, longBaseline, stat.median) > limit(metric, longBaseline)) {
				comparison.drifts.push({ ...change, baseline: longBaseline });
			}
		}
	}

	const last = recent.at(-1);

	if (last && last.lighthouseVersion !== entry.lighthouseVersion) {
		comparison.notes.push(
			`Lighthouse changed from ${last.lighthouseVersion} to ${entry.lighthouseVersion} since the last run.`
		);
	}

	// Lighthouse's estimate of the machine's CPU speed: a slower runner moves every timing.
	const benchmark = recent.length ? median(recent.map((past) => past.benchmarkIndex)) : undefined;

	if (benchmark && Math.abs(entry.benchmarkIndex - benchmark) > 0.25 * benchmark) {
		comparison.notes.push(
			`This machine benchmarked at ${Math.round(entry.benchmarkIndex)}, against ${Math.round(benchmark)} for the baseline's, so timings may have moved with it.`
		);
	}

	return comparison;
}

export const isFailed = (comparison: Comparison) => comparison.regressions.length > 0 || comparison.problems.length > 0;

export function formatValue(metric: Metric, value: number) {
	if (metric.unit === 'score') return String(Math.round(value * 100));

	if (metric.unit === 'bytes') return `${(value / 1024).toFixed(0)} KiB`;

	if (metric.unit === 'ms') return value >= 1000 ? `${(value / 1000).toFixed(2)} s` : `${Math.round(value)} ms`;

	return Number.isInteger(value) ? String(value) : value.toFixed(3);
}

const SPARKS = '▁▂▃▄▅▆▇█';

/** A row of block characters for `values`, from the lowest (▁) to the highest (█). */
export function sparkline(values: number[]) {
	const low = Math.min(...values);
	const range = Math.max(...values) - low;

	return values.map((value) => SPARKS[range ? Math.round(((value - low) / range) * (SPARKS.length - 1)) : 0]).join('');
}

const changeRows = (changes: Change[]) => [
	'| Page | Metric | Baseline | Now |',
	'| --- | --- | ---: | ---: |',
	...changes.map(
		({ key, metric, baseline, current }) =>
			`| \`${key}\` | ${metric.label} | ${formatValue(metric, baseline)} | ${formatValue(metric, current)} |`
	)
];

const TABLE_METRICS = ['performance', 'accessibility', 'best-practices', 'seo', 'lcp', 'tbt', 'cls', 'ttfb', 'bytes'];

/** The comparison as Markdown, for the run's summary or the terminal. `trend` is how many past runs the sparklines show. */
export function renderMarkdown(comparison: Comparison, history: HistoryEntry[], trend = 12) {
	const { entry, regressions, drifts, improvements, problems, unstable, added, notes, baselineRuns } = comparison;

	const lines = [
		'## Lighthouse history',
		'',
		`${entry.commit.replace(/^[0-9a-f]{12}[0-9a-f]*/, (sha) => sha.slice(0, 12))}, Lighthouse ${entry.lighthouseVersion}, against the median of the last ${baselineRuns} recorded run${baselineRuns === 1 ? '' : 's'}.`,
		...notes.map((note) => `\n> ${note}`),
		''
	];

	if (!baselineRuns) lines.push('No history yet: this run is the first baseline.', '');

	if (regressions.length) lines.push(`### Regressions (${regressions.length})`, '', ...changeRows(regressions), '');

	if (problems.length)
		lines.push(
			`### Bad results (${problems.length})`,
			'',
			...problems.map(({ key, problem }) => `- \`${key}\`: ${problem}`),
			''
		);

	if (drifts.length) {
		lines.push(
			`### Slow creep (${drifts.length})`,
			'',
			`Worse than the ${OLDER_RUNS} runs before the baseline's.`,
			'',
			...changeRows(drifts),
			''
		);
	}

	if (unstable.length) {
		lines.push(
			`### Unstable (${unstable.length})`,
			'',
			...unstable.map(
				({ key, metric, stat }) =>
					`- \`${key}\`: ${metric.label} ranged ${formatValue(metric, stat.min)} to ${formatValue(metric, stat.max)}`
			),
			''
		);
	}

	if (improvements.length) lines.push(`### Improvements (${improvements.length})`, '', ...changeRows(improvements), '');

	if (baselineRuns && added.length)
		lines.push(`New pages, with no history yet: ${added.map((key) => `\`${key}\``).join(', ')}`, '');

	const metrics = METRICS.filter((metric) => TABLE_METRICS.includes(metric.id));

	const past = history.slice(-trend);

	const trendOf = (key: string, id: string) =>
		sparkline([...past.flatMap((old) => old.results[key]?.[id] ?? []), entry.results[key][id]]);

	lines.push(
		'<details><summary>Every page</summary>',
		'',
		`| Page | ${metrics.map((metric) => metric.label).join(' | ')} | Performance trend | LCP trend |`,
		`| --- | ${metrics.map(() => '---:').join(' | ')} | --- | --- |`,
		...Object.entries(entry.results).map(
			([key, values]) =>
				`| \`${key}\` | ${metrics.map((metric) => (metric.id in values ? formatValue(metric, values[metric.id]) : '')).join(' | ')} | ${
					'performance' in values ? trendOf(key, 'performance') : ''
				} | ${'lcp' in values ? trendOf(key, 'lcp') : ''} |`
		),
		'',
		'</details>',
		''
	);

	return lines.join('\n');
}
