/**
 * Holds a run of scripts/lighthouse.ts (lighthouse-full/summary-*.json) against the pages' history: a
 * JSON Lines file, one line per recorded run. Prints what regressed, crept, broke or improved, and
 * exits 1 on a regression or a bad result. Plain Node (no dependencies), so CI runs it uninstalled.
 *
 *   node scripts/lighthouse-history.ts                   # compare only, say while bisecting
 *   node scripts/lighthouse-history.ts --record          # compare, then add this run to the history
 *
 * --results <dir>      the summaries' directory (lighthouse-full)
 * --history <file>     the history (lighthouse-history.jsonl); CI keeps it as a workflow artifact
 * --markdown <file>    append the report to a file (CI's $GITHUB_STEP_SUMMARY) instead of printing it
 * --regressions-only   exit 1 on a regression alone, so bad results every commit has don't fail a bisect
 */
import { execFileSync } from 'node:child_process';
import { appendFileSync, existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { compare, type HistoryEntry, isFailed, renderMarkdown, type Summary } from './lib/lighthouse.ts';

const { values: args } = parseArgs({
	options: {
		results: { type: 'string', default: 'lighthouse-full' },
		history: { type: 'string', default: 'lighthouse-history.jsonl' },
		markdown: { type: 'string' },
		record: { type: 'boolean', default: false },
		'regressions-only': { type: 'boolean', default: false }
	}
});

const summaries: Summary[] = readdirSync(args.results)
	.filter((name) => /^summary-.+\.json$/.test(name))
	.map((name) => JSON.parse(readFileSync(join(args.results, name), 'utf8')));

if (!summaries.length) {
	console.error(`No summaries in ${args.results}: run scripts/lighthouse.ts first.`);
	process.exit(1);
}

const history: HistoryEntry[] = existsSync(args.history)
	? readFileSync(args.history, 'utf8')
			.split('\n')
			.filter(Boolean)
			.map((line) => JSON.parse(line))
	: [];

// A run of uncommitted changes says so, so it isn't mistaken for the commit's own numbers.
const git = (...gitArgs: string[]) => execFileSync('git', gitArgs, { encoding: 'utf8' }).trim();

const commit = process.env.GITHUB_SHA ?? `${git('rev-parse', 'HEAD')}${git('status', '--porcelain') ? '-dirty' : ''}`;

const comparison = compare(summaries, history, { date: new Date().toISOString(), commit });

const report = renderMarkdown(comparison, history);

if (args.markdown) appendFileSync(args.markdown, `${report}\n`);
else console.log(report);

if (args.record) appendFileSync(args.history, `${JSON.stringify(comparison.entry)}\n`);

const isFailing = args['regressions-only'] ? comparison.regressions.length > 0 : isFailed(comparison);

if (isFailing) process.exit(1);
