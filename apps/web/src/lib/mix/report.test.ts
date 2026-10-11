import { describe, expect, it } from 'vitest';
import {
	bandColor,
	clock,
	colors,
	handoff,
	keysPlayed,
	lanes,
	niceTicks,
	parseMixReport,
	parseMixReportText,
	phraseAt,
	phraseColor,
	phraseRole,
	speed,
	summarize,
	tracklist
} from './report';
import exported from './fixtures/v2.mix.json' with { type: 'json' };
import { sampleReport } from './sample';

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

describe('display helpers', () => {
	it('formats times and speeds', () => {
		expect(clock(65)).toBe('1:05');
		expect(clock(3725)).toBe('1:02:05');
		expect(speed(1.023)).toBe('+2.3%');
		expect(speed(0.99)).toBe('−1.0%');
	});

	it('summarizes a set', () => {
		const s = summarize(parsed());
		expect(s).toMatchObject({ tracks: 5, confident: 4, coverage: 93, gaps: 1, harmonic: { good: 3, known: 4 } });
		expect(s.tempo).toMatchObject({ median: 126, steady: true });
	});

	it('describes handoffs', () => {
		const [blend, quick, gap, clash] = parsed().transitions.map(handoff);
		expect(blend).toEqual({ how: '30s blend', tempo: '+0.8% tempo', harmony: { ok: true, text: 'same key' } });
		expect(quick.how).toBe('30s blend');
		expect(gap.how).toBe('40s apart');
		expect(gap.tempo).toBe('−1.6% tempo');
		expect(clash.harmony).toEqual({ ok: false, text: 'key clash' });
		expect(handoff({ from: 0, to: 1, at: 0, overlap: 2 }).how).toBe('quick mix');
	});

	it('lays plays out in lanes and lists gaps in order', () => {
		const report = parsed();
		expect(lanes(report.plays)).toEqual([0, 1, 0, 1, 0]);
		const items = tracklist(report);
		expect(items.map((i) => i.kind)).toEqual(['play', 'play', 'play', 'gap', 'play', 'play']);
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
		expect(['Verse 3', 'Phrase 9', 'toString'].map(phraseColor)).toEqual([
			colors.phrases.verse,
			colors.phrases.unknown,
			colors.phrases.unknown
		]);
	});
});
