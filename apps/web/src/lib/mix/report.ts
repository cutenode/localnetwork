/**
 * Mix reports: what a DJ's recorded set plays, when, and how, as exported by locallibrary
 * (`locallibrary mix <set> --export set.mix.json`). The file is untrusted input, so it is
 * validated here before anything renders it: unknown fields are dropped, every list and string
 * is bounded, and images must be small JPEG/PNG/WebP `data:` URLs.
 *
 * Format `locallibrary.mix-report`, versions 1 and 2. Version 2 dropped `library` (the drive's name
 * and track count), so a version 1 report's is dropped here too. Times are seconds into the mix,
 * except a play's `trackStart`/`trackEnd` and a track's cues and phrases, which are seconds into
 * the track.
 */
import type { JsonValue } from '@atproto/lex-json';
import { z } from 'zod';
import colors from '../../../../../packages/tokens/colors.json' with { type: 'json' };

/** Colours that mean the same here as in locallibrary's player and HTML report. */
export { colors };

export const MIX_REPORT_FORMAT = 'locallibrary.mix-report';

/** The newest format version this site reads. */
export const MIX_REPORT_VERSION = 2;

/** Largest export accepted, as uploaded. */
export const MIX_REPORT_MAX_BYTES = 1_000_000;

const DAY = 86_400;

const secs = z.number().finite().min(0).max(DAY);

const byte = z.number().int().min(0).max(255);

const text = (max: number) => z.string().trim().min(1).max(max);

const optionalText = (max: number) =>
	z
		.string()
		.trim()
		.max(max)
		.transform((value) => value || undefined)
		.optional();

const image = z
	.string()
	.max(250_000)
	.regex(/^data:image\/(?:jpeg|png|webp);base64,[A-Za-z0-9+/]+={0,2}$/, 'not a small embedded image');

const color = z.string().regex(/^#[0-9a-fA-F]{6}$/);

const camelot = z.string().regex(/^(?:1[0-2]|[1-9])[AB]$/);

const trackSchema = z.object({
	title: text(300),
	artist: optionalText(300),
	remixer: optionalText(300),
	album: optionalText(300),
	label: optionalText(200),
	genre: optionalText(200),
	year: z.number().int().min(1900).max(2100).optional(),
	key: optionalText(16),
	bpm: z.number().positive().max(400).optional(),
	durationSecs: secs,
	artwork: image.optional(),
	overview: z
		.object({ heights: z.array(byte).max(400), colors: z.array(color).max(400) })
		.refine((o) => o.heights.length === o.colors.length, 'overview heights and colours differ in length')
		.optional(),
	cues: z
		.array(z.object({ at: secs, label: text(40), color: color.optional() }))
		.max(16)
		.default([]),
	phrases: z
		.array(z.object({ start: secs, end: secs, kind: text(24) }))
		.max(128)
		.default([])
});

const playSchema = z.object({
	track: trackSchema,
	start: secs,
	end: secs,
	trackStart: secs,
	trackEnd: secs,
	tempo: z.number().min(0.25).max(4),
	playedBpm: z.number().positive().max(800).optional(),
	keyLock: z.boolean().nullable(),
	semitones: z.number().min(-48).max(48),
	key: camelot.optional(),
	playedKey: camelot.optional(),
	confidence: z.number().min(0).max(1),
	presence: z.array(z.number().int().min(0).max(100)).max(50_000),
	presenceStep: z.number().positive().max(600),
	jumps: z.number().int().min(0).max(10_000)
});

export const mixReportSchema = z
	.object({
		format: z.literal(MIX_REPORT_FORMAT),
		version: z.literal([1, MIX_REPORT_VERSION]),
		generator: z.object({ name: text(64), version: text(32) }),
		analysedAt: z.iso.datetime({ offset: true }),
		mix: z.object({
			title: text(300),
			artist: optionalText(300),
			genre: optionalText(200),
			date: optionalText(64),
			recorder: optionalText(120),
			durationSecs: secs.positive(),
			cover: image.optional()
		}),
		tempoRange: z.number().min(0).max(1),
		coverage: z.number().min(0).max(1),
		waveform: z
			.object({
				secondsPerColumn: z.number().min(0).max(DAY),
				low: z.array(byte).max(5000),
				mid: z.array(byte).max(5000),
				high: z.array(byte).max(5000)
			})
			.refine(
				(w) => w.low.length === w.mid.length && w.mid.length === w.high.length,
				'waveform bands differ in length'
			),
		plays: z.array(playSchema).max(500),
		transitions: z
			.array(
				z.object({
					from: z.number().int().min(0),
					to: z.number().int().min(0),
					at: secs,
					overlap: z.number().min(-DAY).max(DAY),
					harmony: optionalText(32),
					tempoChange: z.number().min(-100).max(100).optional()
				})
			)
			.max(500),
		unidentified: z.array(z.object({ start: secs, end: secs })).max(500)
	})
	.refine((r) => r.transitions.every((t) => t.from < r.plays.length && t.to < r.plays.length), {
		message: 'a transition points at a track that is not in the report'
	});

export type MixReport = z.infer<typeof mixReportSchema>;

export type MixPlay = MixReport['plays'][number];

export type MixTrack = MixPlay['track'];

export type MixTransition = MixReport['transitions'][number];

export type ParseResult = { ok: true; report: MixReport } | { ok: false; error: string };

/** Just enough of a parsed file to tell another kind of file, or a newer format, from a damaged report. */
const reportHeader = z.object({
	format: z.unknown().optional(),
	version: z.number().optional().catch(undefined)
});

/** Validates a parsed export, with an error a DJ can act on. */
export function parseMixReport(value: JsonValue): ParseResult {
	const header = reportHeader.safeParse(value).data;

	// A JSON array, or an object in some other format, is a different kind of file.
	if (Array.isArray(value) || (header && header.format !== MIX_REPORT_FORMAT)) {
		return { ok: false, error: 'This isn’t a mix report from locallibrary (`locallibrary mix … --export`).' };
	}

	if (header?.version !== undefined && header.version > MIX_REPORT_VERSION) {
		return { ok: false, error: `This report uses format version ${header.version}, newer than this site reads.` };
	}

	const result = mixReportSchema.safeParse(value);

	if (result.success) return { ok: true, report: result.data };
	const issue = result.error.issues[0];
	const where = issue?.path.length ? ` (at ${issue.path.join('.')})` : '';

	return { ok: false, error: `This mix report is damaged or incomplete: ${issue?.message ?? 'invalid'}${where}.` };
}

/** Parses an export's raw text (or bytes), enforcing the size limit first. */
export function parseMixReportText(raw: string | Uint8Array): ParseResult {
	const size = ArrayBuffer.isView(raw) ? raw.length : new TextEncoder().encode(raw).length;

	if (size > MIX_REPORT_MAX_BYTES) return { ok: false, error: 'Mix reports can be at most 1 MB.' };
	let json: JsonValue;

	try {
		json = JSON.parse(ArrayBuffer.isView(raw) ? new TextDecoder().decode(raw) : raw);
	} catch {
		return { ok: false, error: 'That file isn’t JSON. Export it with `locallibrary mix … --export set.mix.json`.' };
	}

	return parseMixReport(json);
}

// ---------------------------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------------------------

/** `4:05`, or `1:04:05` past an hour. */
export function clock(seconds: number): string {
	const s = Math.max(0, Math.round(seconds));
	const h = Math.floor(s / 3600);
	const m = Math.floor((s % 3600) / 60);
	const x = String(s % 60).padStart(2, '0');

	return h ? `${h}:${String(m).padStart(2, '0')}:${x}` : `${m}:${x}`;
}

/** A playback speed as a signed percentage: `+2.3%`, `−1.0%`. */
export function speed(tempo: number): string {
	const v = (tempo - 1) * 100;

	return `${v >= 0 ? '+' : '−'}${Math.abs(v).toFixed(1)}%`;
}

/** Plays at or above this are shown as certain. */
export const CONFIDENT = 0.5;

export type Summary = {
	tracks: number;
	confident: number;
	coverage: number;
	gaps: number;
	tempo: { median: number; low: number; high: number; steady: boolean } | null;
	harmonic: { good: number; known: number };
};

export function summarize(report: MixReport): Summary {
	const bpms = report.plays
		.map((p) => p.playedBpm)
		.filter((b): b is number => b !== undefined)
		.sort((a, b) => a - b);

	const known = report.transitions.filter((t) => t.harmony);

	return {
		tracks: report.plays.length,
		confident: report.plays.filter((p) => p.confidence >= CONFIDENT).length,
		coverage: Math.round(report.coverage * 100),
		gaps: report.unidentified.length,
		tempo: bpms.length
			? {
					median: bpms[Math.floor(bpms.length / 2)],
					low: bpms[0],
					high: bpms[bpms.length - 1],
					steady: Math.round(bpms[bpms.length - 1]) - Math.round(bpms[0]) < 2
				}
			: null,
		harmonic: { good: known.filter((t) => t.harmony !== 'clash').length, known: known.length }
	};
}

export type Handoff = { how: string; tempo: string | null; harmony: { ok: boolean; text: string } | null };

/** How one track hands over to the next, in words. */
export function handoff(t: MixTransition): Handoff {
	// Fades below what fingerprints hear make short "gaps"; only a long one is called a gap.
	const how =
		t.overlap >= 8
			? `${Math.round(t.overlap)}s blend`
			: t.overlap > -12
				? 'quick mix'
				: `${Math.round(-t.overlap)}s apart`;

	const tempo =
		t.tempoChange !== undefined && Math.abs(t.tempoChange) >= 0.3
			? `${t.tempoChange > 0 ? '+' : '−'}${Math.abs(t.tempoChange).toFixed(1)}% tempo`
			: null;

	const harmony = t.harmony
		? t.harmony === 'clash'
			? { ok: false, text: 'key clash' }
			: { ok: true, text: t.harmony }
		: null;

	return { how, tempo, harmony };
}

/** Which lane each play sits in: alternating decks, a third only when three overlap. */
export function lanes(plays: MixPlay[]): number[] {
	const ends: number[] = [];

	return plays.map((p, i) => {
		const free = (lane: number) => (ends[lane] ?? -Infinity) <= p.start + 0.5;
		let lane = i % 2;

		if (!free(lane)) {
			lane = ends.findIndex((_, l) => free(l));

			if (lane < 0) lane = ends.length;
		}

		ends[lane] = p.end;

		return lane;
	});
}

/** Indexes of the plays heard at `t`. */
export function playingAt(report: MixReport, t: number): number[] {
	return report.plays.flatMap((p, i) => (p.start <= t && t <= p.end ? [i] : []));
}

export type TracklistItem =
	| { kind: 'play'; index: number; play: MixPlay }
	| { kind: 'gap'; start: number; end: number };

/** Plays and unidentified stretches, in mix order. */
export function tracklist(report: MixReport): TracklistItem[] {
	const items: (TracklistItem & { at: number })[] = [
		...report.plays.map((play, index) => ({ kind: 'play' as const, index, play, at: play.start })),
		...report.unidentified.map((u) => ({ kind: 'gap' as const, start: u.start, end: u.end, at: u.start }))
	];

	return items.sort((a, b) => a.at - b.at).map(({ at: _at, ...item }) => item);
}

/** The phrase a track is in at `seconds` into it. */
export function phraseAt(track: MixTrack, seconds: number): string | undefined {
	return track.phrases.find((p) => p.start <= seconds && seconds < p.end)?.kind;
}

export type Camelot = { n: number; major: boolean };

export function parseCamelot(key: string | undefined): Camelot | null {
	const m = /^(\d{1,2})([AB])$/.exec(key ?? '');

	return m ? { n: Number(m[1]), major: m[2] === 'B' } : null;
}

/** Plays by the key they were heard in, in order of first use. */
export function keysPlayed(report: MixReport): Map<string, number[]> {
	const out = new Map<string, number[]>();
	report.plays.forEach((p, i) => {
		if (!parseCamelot(p.playedKey)) return;
		out.set(p.playedKey!, [...(out.get(p.playedKey!) ?? []), i]);
	});

	return out;
}

/** Round axis values between `low` and `high`, about `count` of them. */
export function niceTicks(low: number, high: number, count: number): number[] {
	const span = high - low;

	if (!(span > 0)) return [low];
	const raw = span / count;
	const mag = 10 ** Math.floor(Math.log10(raw));
	const step = [1, 2, 5, 10].map((m) => m * mag).find((s) => span / s <= count) ?? mag * 10;
	const out: number[] = [];

	for (let v = Math.ceil(low / step) * step; v <= high + 1e-9; v += step) out.push(Number(v.toFixed(6)));

	return out;
}

/** Seconds between time labels so about `target` fit along `duration`. */
export function timeStep(duration: number, target: number): number {
	return [30, 60, 120, 300, 600, 900, 1200, 1800].find((s) => duration / s <= target) ?? 3600;
}

/**
 * rekordbox's colour waveforms draw lows blue, mids orange and highs white. Snapped to the mix
 * waveform's own band colours, a track's overview reads the same way as the mix above it.
 */
export function bandColor(hex: string): string {
	const n = Number.parseInt(hex.slice(1), 16);
	const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];

	if (Math.min(r, g, b) >= 160) return colors.bands.high;

	return b > r ? colors.bands.low : colors.bands.mid;
}

export type PhraseRole = keyof typeof colors.phrases;

const isPhraseRole = (word: string): word is PhraseRole => Object.hasOwn(colors.phrases, word);

/** What a phrase kind does in the track: `Verse 2` is a verse, a kind rekordbox doesn't name is unknown. */
export function phraseRole(kind: string): PhraseRole {
	const word = kind.split(' ')[0].toLowerCase();

	return isPhraseRole(word) ? word : 'unknown';
}

/** A phrase's colour, by its role, as the player and locallibrary's report draw it. */
export function phraseColor(kind: string): string {
	return colors.phrases[phraseRole(kind)];
}
