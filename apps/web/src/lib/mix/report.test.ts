import type { JsonValue } from '@atproto/lex-json';
import * as fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import {
	BLEND_SECS,
	bandColor,
	bpmAt,
	clock,
	colors,
	handoff,
	keyPath,
	keysPlayed,
	lanes,
	niceTicks,
	parseCamelot,
	phraseAt,
	phraseBackground,
	phraseRole,
	playColor,
	playSpeed,
	speed,
	summarize,
	tempoSteps,
	timeStep,
	tracklist
} from './report';
import { type ParseResult, parseMixReport, parseMixReportText } from './schema';
import exported from './fixtures/v2.mix.json' with { type: 'json' };
import { riddenReport, sampleReport } from './sample';

const parsed = () => {
	const result = parseMixReport(structuredClone(sampleReport));

	if (!result.ok) throw new Error(result.error);

	return result.report;
};

describe('parseMixReport', () => {
	it('accepts the sample and drops unknown fields', () => {
		const report = parseMixReport({
			...structuredClone(sampleReport),
			secret: 'x',
			mix: { ...sampleReport.mix, path: '/Users/me/set.wav' }
		});

		expect(report.ok).toBe(true);

		if (!report.ok) return;
		expect(report.report.plays).toHaveLength(5);
		expect(JSON.stringify(report.report)).not.toContain('/Users/me');
		expect('secret' in report.report).toBe(false);
	});

	// The fixture is what `locallibrary mix … --export` writes for a mix of rekordcrate's two demo
	// tracks. Regenerate it with `pnpm fixtures:mix` (scripts/mix-fixture.ts) when the format changes.
	it('reads version 2, which has no library', () => {
		expect(parseMixReportText(JSON.stringify(exported))).toMatchObject({
			ok: true,
			report: { version: 2, plays: [{ track: { title: 'Demo Track 1' } }, { track: { title: 'Demo Track 2' } }] }
		});
	});

	it('reads version 1 and drops the library it names', () => {
		const report = parseMixReport({ ...exported, version: 1, library: { name: 'Main', tracks: 812 } });

		expect(report).toMatchObject({ ok: true, report: { version: 1 } });
		expect(report.ok && 'library' in report.report).toBe(false);
	});

	it('drops optional text that is only spaces', () => {
		const report = parseMixReport({ ...structuredClone(sampleReport), mix: { ...sampleReport.mix, artist: '   ' } });

		expect(report).toMatchObject({ ok: true, report: { mix: { title: 'Sunday Warm-up' } } });
		expect(report.ok && report.report.mix.artist).toBeUndefined();
	});

	it('rejects versions newer than 2', () => {
		expect(parseMixReport({ ...exported, version: 3 })).toEqual({
			ok: false,
			error: 'This report uses format version 3, newer than this site reads.'
		});
	});

	it('explains what is wrong', () => {
		expect(parseMixReport({ format: 'something else' })).toMatchObject({
			ok: false,
			error: expect.stringContaining('locallibrary')
		});
		expect(parseMixReport({ ...sampleReport, version: 7 })).toMatchObject({
			ok: false,
			error: expect.stringContaining('version 7')
		});
		const broken = structuredClone(sampleReport);
		broken.transitions[0].to = 99;
		expect(parseMixReport(broken)).toMatchObject({ ok: false, error: expect.stringContaining('transition') });
		const script = structuredClone(sampleReport);
		script.mix.cover = 'javascript:alert(1)';
		expect(parseMixReport(script).ok).toBe(false);
	});

	it('tells other files from damaged reports', () => {
		const notReport = 'This isn’t a mix report from locallibrary (`locallibrary mix … --export`).';
		expect(parseMixReport([])).toEqual({ ok: false, error: notReport });
		expect(parseMixReport({ version: 1 })).toEqual({ ok: false, error: notReport });
		expect(parseMixReport(null)).toEqual({
			ok: false,
			error: 'This mix report is damaged or incomplete: Invalid input: expected object, received null.'
		});
		expect(parseMixReport('report')).toEqual({
			ok: false,
			error: 'This mix report is damaged or incomplete: Invalid input: expected object, received string.'
		});
		expect(parseMixReport({ ...sampleReport, version: '7' })).toMatchObject({
			ok: false,
			error: expect.stringMatching(/^This mix report is damaged or incomplete: .* \(at version\)\.$/)
		});
		expect(parseMixReport({ ...sampleReport, version: 0 })).toMatchObject({
			ok: false,
			error: expect.stringContaining('(at version)')
		});
	});

	it('enforces the size limit and JSON', () => {
		expect(parseMixReportText('x'.repeat(1_000_001))).toMatchObject({
			ok: false,
			error: expect.stringContaining('1 MB')
		});
		expect(parseMixReportText('{nope')).toMatchObject({ ok: false, error: expect.stringContaining('JSON') });
		expect(parseMixReportText(JSON.stringify(sampleReport)).ok).toBe(true);
		expect(parseMixReportText(new TextEncoder().encode(JSON.stringify(sampleReport))).ok).toBe(true);
	});
});

describe('parseMixReport on any input', () => {
	/** Where a value sits in a report: keys, and list indexes as strings, from the top. */
	type Path = string[];

	/** A list or an object, read by key alike: a list's keys are its indexes. */
	type Container = { [key: string]: JsonValue | undefined };

	// `Object(value) === value` holds only for lists and objects, the values with places inside them.
	const isContainer = (value: JsonValue | undefined): value is Container => Object(value) === value;

	/**
	 * Every place in `value`, reading only the first two items of a list so the waveform's thousands of numbers don't
	 * crowd out the fields.
	 */
	const paths = (value: JsonValue | undefined, at: Path = []): Path[] => {
		if (!isContainer(value)) return [at];

		const entries = Array.isArray(value) ? Object.entries(value).slice(0, 2) : Object.entries(value);

		return [at, ...entries.flatMap(([key, item]) => paths(item, [...at, key]))];
	};

	/** What sits at `path` in `value`, if anything does. */
	const valueAt = (value: JsonValue, path: Path) =>
		path.reduce<JsonValue | undefined>((node, key) => (isContainer(node) ? node[key] : undefined), value);

	/** A copy of `report` with each change made: a new value at a path, or the path removed. */
	const mutate = (report: JsonValue, changes: [Path, JsonValue | undefined][]): JsonValue => {
		const copy = structuredClone(report);

		for (const [path, value] of changes) {
			const parent = valueAt(copy, path.slice(0, -1));
			const key = path[path.length - 1];

			if (!isContainer(parent)) continue;

			if (value !== undefined) parent[key] = value;
			else if (Array.isArray(parent)) parent.splice(Number(key), 1);
			else delete parent[key];
		}

		return copy;
	};

	// Changes to the exporter's own output and to the richer sample: any field to an odd value or removed, and text to
	// other text, blank or padded, which is where trimming and length limits show. Text changes come three times as
	// often, and whitespace-only text a third of the time, since most odd values make the whole report invalid and hide
	// a blank title behind an error.
	const mutatedReport = fc.constantFrom<JsonValue>(exported, structuredClone(sampleReport)).chain((report) => {
		const all = paths(report).filter((path) => path.length > 0);
		const text = all.filter((path) => z.string().safeParse(valueAt(report, path)).success);

		const change = fc.oneof(
			fc.tuple(
				fc.constantFrom(...all),
				fc.option(fc.oneof(fc.jsonValue({ maxDepth: 2 }), fc.constantFrom<JsonValue>(-1, 0, 1e9, null, [], {})), {
					nil: undefined
				})
			),
			{
				arbitrary: fc.tuple(
					fc.constantFrom(...text),
					fc.oneof(fc.constantFrom('', '   ', ' padded ', '#zzzzzz', '13C'), fc.stringMatching(/^\s+$/), fc.string())
				),
				weight: 3
			}
		);

		return fc.array(change, { minLength: 1, maxLength: 3 }).map((changes) => mutate(report, changes));
	});

	// The errors a DJ is shown.
	const knownError = new RegExp(
		[
			/^This isn’t a mix report from locallibrary /,
			/^This report uses format version .+, newer than this site reads\.$/,
			/^This mix report is damaged or incomplete: .+\.$/,
			/^That file isn’t JSON\. /,
			/^Mix reports can be at most 1 MB\.$/
		]
			.map((error) => error.source)
			.join('|')
	);

	/** A result as the site reads it back: a report stored as JSON and parsed again, an error as it was. */
	const readBack = (result: ParseResult): ParseResult =>
		result.ok ? parseMixReportText(JSON.stringify(result.report)) : result;

	/**
	 * What a parse should settle to: one of the errors a DJ is shown, or a report that reads back the same once stored,
	 * since the site stores the report it parsed and parses that again when it shows it. Stored as JSON, which writes -0
	 * as 0.
	 */
	const settled = (result: ParseResult) =>
		result.ok
			? { ok: true, report: JSON.parse(JSON.stringify(result.report)) }
			: { ok: false, error: expect.stringMatching(knownError) };

	it('settles any JSON value', () => {
		fc.assert(
			fc.property(fc.jsonValue(), (value) => {
				const result = parseMixReport(value);

				expect(readBack(result)).toEqual(settled(result));
			})
		);
	});

	it('settles any text or bytes', () => {
		fc.assert(
			fc.property(
				fc.oneof(
					fc.string(),
					fc.jsonValue().map((value) => JSON.stringify(value)),
					fc.uint8Array()
				),
				(raw) => {
					const result = parseMixReportText(raw);

					expect(readBack(result)).toEqual(settled(result));
				}
			)
		);
	});

	it('settles any change to a valid report', () => {
		fc.assert(
			fc.property(mutatedReport, (report) => {
				for (const result of [parseMixReport(report), parseMixReportText(JSON.stringify(report))])
					expect(readBack(result)).toEqual(settled(result));
			})
		);
	});
});

describe('display helpers', () => {
	it('formats times and speeds', () => {
		expect(clock(65)).toBe('1:05');
		expect(clock(3725)).toBe('1:02:05');
		expect(speed(1.023)).toBe('+2.3%');
		expect(speed(0.99)).toBe('−1.0%');
	});

	it('gives a ridden pitch fader its slowest and fastest speeds', () => {
		expect(playSpeed({ tempo: 1.02, speedRange: [1, 1.04] })).toBe('+0.0% to +4.0%');
		expect(playSpeed({ tempo: 1.02 })).toBe('+2.0%');
		const report = structuredClone(sampleReport);
		report.plays[0] = { ...report.plays[0], speedRange: [0.99, 1.03] };
		const result = parseMixReport(report);
		expect(result.ok && result.report.plays[0].speedRange).toEqual([0.99, 1.03]);
	});

	it('summarizes a set', () => {
		const s = summarize(parsed());
		expect(s).toMatchObject({ tracks: 5, confident: 4, coverage: 93, gaps: 1, harmonic: { good: 3, known: 4 } });
		expect(s.tempo).toMatchObject({ median: 126, steady: true });
	});

	it('steps a ridden track between its speeds, and the summary takes in both ends', () => {
		const result = parseMixReport(structuredClone(riddenReport));

		if (!result.ok) throw new Error(result.error);

		const steps = tempoSteps(result.report.plays[1]);
		expect(steps.map((s) => [s.from, s.to, +s.bpm.toFixed(2)])).toEqual([
			[300, 360, 124],
			[360, 420, 133.92],
			[420, 640, 133.92]
		]);
		expect([bpmAt(steps, 200), bpmAt(steps, 359), bpmAt(steps, 500)].map((b) => b?.toFixed(2))).toEqual([
			'124.00',
			'124.00',
			'133.92'
		]);
		expect(summarize(result.report).tempo).toMatchObject({ low: 124, high: expect.closeTo(133.92), steady: false });
		// A steady play is one step at its BPM, and one without a BPM has none.
		expect(tempoSteps(result.report.plays[0])).toEqual([{ from: 0, to: result.report.plays[0].end, bpm: 126 }]);
		expect(tempoSteps({ ...result.report.plays[0], playedBpm: undefined })).toEqual([]);
	});

	it('works out the BPM a ridden track was recorded at when the library has none', () => {
		const ridden = riddenReport.plays[1];
		const steps = tempoSteps({ ...ridden, track: { ...ridden.track, bpm: undefined } });

		// 128.96 BPM played at +4% was recorded at 124 BPM.
		expect(steps.map((s) => +s.bpm.toFixed(2))).toEqual([124, 133.92, 133.92]);
	});

	it('has no tempo to sum up when no play has a BPM', () => {
		const report = parsed();
		report.plays = report.plays.map((play) => ({ ...play, playedBpm: undefined }));

		expect(summarize(report).tempo).toBeNull();
	});

	it('describes handoffs', () => {
		const [blend, quick, gap, clash] = parsed().transitions.map(handoff);
		expect(blend).toEqual({ how: '30s blend', tempo: '+0.8% tempo', harmony: { ok: true, text: 'same key' } });
		expect(quick.how).toBe('30s blend');
		expect(gap.how).toBe('40s apart');
		expect(gap.tempo).toBe('−1.6% tempo');
		expect(clash.harmony).toEqual({ ok: false, text: 'key clash' });
		expect(handoff({ from: 0, to: 1, at: 0, overlap: 2 }).how).toBe('quick mix');
		expect(handoff({ from: 0, to: 1, at: 0, overlap: 2, tempoChange: 0.2 })).toEqual({
			how: 'quick mix',
			tempo: null,
			harmony: null
		});
	});

	it('calls a transition a blend from the threshold locallibrary uses', () => {
		expect(handoff({ from: 0, to: 1, at: 0, overlap: BLEND_SECS }).how).toBe(`${BLEND_SECS}s blend`);
		expect(handoff({ from: 0, to: 1, at: 0, overlap: BLEND_SECS - 0.1 }).how).toBe('quick mix');
	});

	it('lays plays out in lanes and lists gaps in order', () => {
		const report = parsed();
		expect(lanes(report.plays)).toEqual([0, 1, 0, 1, 0]);
		const items = tracklist(report);
		expect(items.map((i) => i.kind)).toEqual(['play', 'play', 'play', 'gap', 'play', 'play']);
	});

	it('opens a third lane only when both decks are busy, and reuses whichever deck frees up', () => {
		const at = (start: number, end: number) => ({ ...parsed().plays[0], start, end });

		expect(lanes([at(0, 100), at(10, 100), at(20, 30), at(150, 200), at(160, 170), at(165, 180)])).toEqual([
			0, 1, 2, 1, 0, 2
		]);
	});

	it('never puts two plays that overlap by more than half a second in one lane', () => {
		// Parsed once: parsing the whole report for every span is what makes a run slow.
		const [base] = parsed().plays;

		fc.assert(
			fc.property(fc.array(fc.tuple(fc.nat({ max: 7200 }), fc.nat({ max: 600 }))), (spans) => {
				const plays = spans
					.toSorted(([a], [b]) => a - b)
					.map(([start, length]) => ({ ...base, start, end: start + length }));

				const laid = lanes(plays);

				plays.forEach((play, i) => {
					const before = plays.slice(0, i).filter((_, k) => laid[k] === laid[i]);
					expect(before.every((other) => other.end <= play.start + 0.5)).toBe(true);
				});
			})
		);
	});

	it('reads keys, phrases and colours', () => {
		const report = parsed();
		expect([...keysPlayed(report).entries()]).toEqual([
			['8A', [0, 1]],
			['9A', [2]],
			['10A', [3]],
			['3B', [4]]
		]);
		expect(phraseAt(report.plays[0].track, 70)).toBe('Up');
		expect(niceTicks(118, 132, 4)).toEqual([120, 125, 130]);
	});

	it('skips plays with no key heard, or one off the Camelot wheel', () => {
		const report = parsed();
		report.plays[1].playedKey = undefined;
		report.plays[2].playedKey = 'C#m';

		expect([...keysPlayed(report).keys()]).toEqual(['8A', '10A', '3B']);
	});

	it('reads Camelot keys, and nothing else', () => {
		expect(['8A', '12B', 'C#m', '', undefined].map(parseCamelot)).toEqual([
			{ n: 8, major: false },
			{ n: 12, major: true },
			null,
			null,
			null
		]);
	});

	it('gives a flat range a single tick', () => {
		expect(niceTicks(124, 124, 4)).toEqual([124]);
		expect(niceTicks(130, 120, 4)).toEqual([130]);
	});

	// From three ticks asked for, the step is shorter than the range, so a multiple of it always falls inside.
	it('always gives round ticks in order within the range', () => {
		fc.assert(
			fc.property(
				fc.double({ min: -1e6, max: 1e6, noNaN: true }),
				fc.double({ min: 1e-3, max: 1e6, noNaN: true }),
				fc.integer({ min: 3, max: 12 }),
				(low, span, count) => {
					const ticks = niceTicks(low, low + span, count);

					expect(ticks.length).toBeGreaterThan(0);
					expect(ticks.length).toBeLessThanOrEqual(count + 1);
					expect(ticks.toSorted((a, b) => a - b)).toEqual(ticks);
					expect(ticks[0]).toBeGreaterThanOrEqual(low - 1e-6 * Math.max(1, Math.abs(low)));
					expect(ticks.at(-1)).toBeLessThanOrEqual(low + span + 1e-6 * Math.max(1, Math.abs(low + span)));
				}
			)
		);
	});

	it('spaces time labels by round steps, hourly past what the steps fit', () => {
		expect(timeStep(1740, 7)).toBe(300);
		expect(timeStep(120, 4)).toBe(30);
		expect(timeStep(10 * 3600, 4)).toBe(3600);
	});

	// rekordbox's overview colours, snapped to the mix waveform's bands.
	it('snaps overview colours to the band colours', () => {
		const { low, mid, high } = colors.bands;

		expect(['#2050ff', '#0000bf', '#ff7a1a', '#bf0904', '#f0f0f0'].map(bandColor)).toEqual([low, low, mid, mid, high]);
	});

	it('colours phrases by what they do, as the player does', () => {
		const kinds = ['Intro', 'Up', 'Verse 1', 'Verse 6', 'Chorus', 'Down', 'Bridge', 'Outro', 'Phrase 9', ''];

		expect(kinds.map(phraseRole)).toEqual([
			'intro',
			'up',
			'verse',
			'verse',
			'chorus',
			'down',
			'bridge',
			'outro',
			'unknown',
			'unknown'
		]);
		expect(['Verse 3', 'Phrase 9', 'toString'].map(phraseBackground)).toEqual([
			'bg-phrase-verse',
			'bg-phrase-unknown',
			'bg-phrase-unknown'
		]);
	});
});

describe('keyPath and playColor', () => {
	it('lists the keys heard in order, merging repeats in a row', () => {
		expect(keyPath(parsed())).toEqual(['8A', '9A', '10A', '3B']);
		const report = parsed();
		report.plays[2].playedKey = undefined;
		report.plays[3].playedKey = '9A';
		expect(keyPath(report)).toEqual(['8A', '9A', '3B']);
	});

	it('gives neighbouring plays different colours, none red or green', () => {
		const used = Array.from({ length: 6 }, (_, i) => playColor(i));
		expect(new Set(used).size).toBe(6);
		expect(used).not.toContain(colors.trackColors.red);
		expect(used).not.toContain(colors.trackColors.green);
		expect(playColor(6)).toBe(playColor(0));
	});
});
