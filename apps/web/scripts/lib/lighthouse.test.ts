import { describe, expect, it } from 'vitest';
import {
	compare,
	formatValue,
	type HistoryEntry,
	isFailed,
	type Lhr,
	METRICS,
	median,
	OLDER_RUNS,
	RECENT_RUNS,
	renderMarkdown,
	sparkline,
	summarizePage,
	type Summary
} from './lighthouse.ts';

const metric = (id: string) => METRICS.find((candidate) => candidate.id === id)!;

/** A run of `path` that scored `performance` with an LCP of `lcp` ms. */
function lhr(path: string, { performance = 0.98, lcp = 1800, benchmarkIndex = 2000 } = {}): Lhr {
	return {
		lighthouseVersion: '12.6.1',
		finalDisplayedUrl: `http://127.0.0.1:5198${path}`,
		environment: { benchmarkIndex },
		categories: { performance: { score: performance }, accessibility: { score: 1 } },
		audits: {
			'largest-contentful-paint': { score: 0.9, numericValue: lcp },
			'http-status-code': { score: 1 }
		}
	};
}

const summary = (pages: Summary['pages'], formFactor: Summary['formFactor'] = 'mobile'): Summary => ({
	formFactor,
	lighthouseVersion: '12.6.1',
	benchmarkIndex: 2000,
	runs: 3,
	pages
});

const page = (path: string, metrics: Record<string, number>, problems: string[] = []) => ({
	path,
	runs: 3,
	problems,
	metrics: Object.fromEntries(
		Object.entries(metrics).map(([id, value]) => [id, { median: value, min: value, max: value }])
	)
});

const entry = (results: HistoryEntry['results'], extra: Partial<HistoryEntry> = {}): HistoryEntry => ({
	date: '2026-10-01T00:00:00.000Z',
	commit: 'abc',
	lighthouseVersion: '12.6.1',
	benchmarkIndex: 2000,
	results,
	...extra
});

const RUN = { date: '2026-10-31T00:00:00.000Z', commit: '0123456789abcdef' };

describe('median', () => {
	it('takes the middle value, or the mean of the two middle ones', () => {
		expect(median([3, 1, 2])).toBe(2);
		expect(median([4, 1, 3, 2])).toBe(2.5);
	});
});

describe('summarizePage', () => {
	it('keeps the median, lowest and highest of each metric over the runs', () => {
		const result = summarizePage({ path: '/' }, [
			lhr('/', { lcp: 1700 }),
			lhr('/', { lcp: 2100 }),
			lhr('/', { lcp: 1800 })
		]);

		expect(result.runs).toBe(3);
		expect(result.problems).toEqual([]);
		expect(result.metrics.lcp).toEqual({ median: 1800, min: 1700, max: 2100 });
		expect(result.metrics.accessibility).toEqual({ median: 1, min: 1, max: 1 });
		expect(result.metrics).not.toHaveProperty('tbt');
	});

	it('reports failed runs and leaves them out of the numbers', () => {
		const failed = { ...lhr('/', { lcp: 9000 }), runtimeError: { code: 'NO_FCP', message: 'The page did not paint' } };
		const result = summarizePage({ path: '/' }, [failed, lhr('/', { lcp: 1800 })]);

		expect(result.runs).toBe(1);
		expect(result.problems).toEqual(['a run failed: NO_FCP (The page did not paint)']);
		expect(result.metrics.lcp.median).toBe(1800);
	});

	it('reports a page that redirected, answered an error, or never ran', () => {
		const redirected = { ...lhr('/login?next=%2Fdashboard'), audits: { 'http-status-code': { score: 0 } } };

		expect(summarizePage({ path: '/dashboard', account: 'dj' }, [redirected]).problems).toEqual([
			'ended up on /login?next=%2Fdashboard',
			'answered with an error status'
		]);
		expect(summarizePage({ path: '/' }, []).problems).toEqual(['no run produced a result']);
	});

	it('adds lhci assertion failures to the problems', () => {
		expect(summarizePage({ path: '/' }, [lhr('/')], ['failed largest-contentful-paint']).problems).toEqual([
			'failed largest-contentful-paint'
		]);
	});
});

describe('compare', () => {
	const history = Array.from({ length: RECENT_RUNS }, () => entry({ 'mobile /': { performance: 0.98, lcp: 1800 } }));

	it('flags a metric worse than the recent median by more than its tolerance', () => {
		const result = compare([summary([page('/', { performance: 0.9, lcp: 2400 })])], history, RUN);

		expect(result.regressions.map(({ metric: { id }, baseline, current }) => [id, baseline, current])).toEqual([
			['performance', 0.98, 0.9],
			['lcp', 1800, 2400]
		]);
		expect(isFailed(result)).toBe(true);
	});

	it('leaves changes within tolerance alone, in both directions', () => {
		// LCP's tolerance is the larger of 150 ms and 10%: 180 ms on 1.8 s.
		const result = compare([summary([page('/', { performance: 0.96, lcp: 1970 })])], history, RUN);

		expect(result.regressions).toEqual([]);
		expect(result.improvements).toEqual([]);
		expect(isFailed(result)).toBe(false);
	});

	it('lists improvements past the tolerance', () => {
		const result = compare([summary([page('/', { performance: 1, lcp: 1200 })])], history, RUN);

		expect(result.improvements.map(({ metric: { id } }) => id)).toEqual(['lcp']);
	});

	it('takes the median of the recent runs, so one outlier in them moves nothing', () => {
		const withOutlier = [...history.slice(1), entry({ 'mobile /': { lcp: 6000 } })];

		expect(compare([summary([page('/', { lcp: 1850 })])], withOutlier, RUN).regressions).toEqual([]);
	});

	it('flags a slow creep against the runs before the recent ones', () => {
		const older = Array.from({ length: OLDER_RUNS }, () => entry({ 'mobile /': { lcp: 1500 } }));
		const recent = [1600, 1700, 1750, 1800, 1850].map((lcp) => entry({ 'mobile /': { lcp } }));
		const result = compare([summary([page('/', { lcp: 1800 })])], [...older, ...recent], RUN);

		expect(result.regressions).toEqual([]);
		expect(result.drifts.map(({ metric: { id }, baseline }) => [id, baseline])).toEqual([['lcp', 1500]]);
		expect(isFailed(result)).toBe(false);
	});

	it('keeps form factors apart', () => {
		const result = compare([summary([page('/', { lcp: 2400 })], 'desktop')], history, RUN);

		expect(result.regressions).toEqual([]);
		expect(result.added).toEqual(['desktop /']);
	});

	it('fails on a bad result even with nothing to compare against', () => {
		const result = compare([summary([page('/dashboard', {}, ['ended up on /login'])])], [], RUN);

		expect(result.problems).toEqual([{ key: 'mobile /dashboard', problem: 'ended up on /login' }]);
		expect(result.baselineRuns).toBe(0);
		expect(isFailed(result)).toBe(true);
	});

	it('flags runs that spread wider than twice the tolerance', () => {
		const spread = { ...page('/', {}), metrics: { lcp: { median: 1800, min: 1500, max: 2400 } } };

		expect(compare([summary([spread])], history, RUN).unstable.map(({ key, metric: { id } }) => [key, id])).toEqual([
			['mobile /', 'lcp']
		]);
	});

	it('notes a new Lighthouse version and a machine of a different speed', () => {
		const old = history.map((past) => ({ ...past, lighthouseVersion: '12.5.0', benchmarkIndex: 1000 }));

		expect(compare([summary([page('/', { lcp: 1800 })])], old, RUN).notes).toEqual([
			'Lighthouse changed from 12.5.0 to 12.6.1 since the last run.',
			"This machine benchmarked at 2000, against 1000 for the baseline's, so timings may have moved with it."
		]);
	});

	it('records each page’s medians under its form factor and path', () => {
		const result = compare(
			[summary([page('/', { lcp: 1800 })]), summary([page('/', { lcp: 900 })], 'desktop')],
			[],
			RUN
		);

		expect(result.entry).toEqual({
			...RUN,
			lighthouseVersion: '12.6.1',
			benchmarkIndex: 2000,
			results: { 'mobile /': { lcp: 1800 }, 'desktop /': { lcp: 900 } }
		});
	});
});

describe('formatValue', () => {
	it('shows each unit the way Lighthouse does', () => {
		expect(formatValue(metric('performance'), 0.973)).toBe('97');
		expect(formatValue(metric('lcp'), 1834)).toBe('1.83 s');
		expect(formatValue(metric('tbt'), 42.4)).toBe('42 ms');
		expect(formatValue(metric('bytes'), 204_800)).toBe('200 KiB');
		expect(formatValue(metric('cls'), 0.0123)).toBe('0.012');
		expect(formatValue(metric('dom'), 812)).toBe('812');
	});
});

describe('sparkline', () => {
	it('draws the lowest value lowest and the highest highest', () => {
		expect(sparkline([1, 8, 4.5])).toBe('▁█▅');
		expect(sparkline([3, 3])).toBe('▁▁');
	});
});

describe('renderMarkdown', () => {
	it('puts regressions and bad results first, and every page in a table', () => {
		const history = [entry({ 'mobile /': { performance: 0.98, lcp: 1800 } })];

		const result = compare(
			[summary([page('/', { performance: 0.98, lcp: 2400 }), page('/admin', {}, ['ended up on /login'])])],
			history,
			RUN
		);

		const markdown = renderMarkdown(result, history);

		expect(markdown).toContain('0123456789ab, Lighthouse 12.6.1, against the median of the last 1 recorded run.');
		expect(markdown).toContain(
			'### Regressions (1)\n\n| Page | Metric | Baseline | Now |\n| --- | --- | ---: | ---: |\n| `mobile /` | LCP | 1.80 s | 2.40 s |'
		);
		expect(markdown).toContain('### Bad results (1)\n\n- `mobile /admin`: ended up on /login');
		expect(markdown).toContain('New pages, with no history yet: `mobile /admin`');
		expect(markdown).toContain('| `mobile /` | 98 |  |  |  | 2.40 s |  |  |  |  | ▁▁ | ▁█ |');
	});

	it('shortens the commit but keeps a dirty marker', () => {
		const dirty = { ...RUN, commit: `${RUN.commit}0123456789abcdef01234567-dirty` };

		expect(renderMarkdown(compare([summary([page('/', { lcp: 1800 })])], [], dirty), [])).toContain(
			'0123456789ab-dirty, Lighthouse'
		);
	});

	it('says when there is no history yet, without listing every page as new', () => {
		const markdown = renderMarkdown(compare([summary([page('/', { lcp: 1800 })])], [], RUN), []);

		expect(markdown).toContain('No history yet: this run is the first baseline.');
		expect(markdown).not.toContain('New pages');
	});
});
