/**
 * The mix report's schema and parser (see report.ts for the format). The file is untrusted input, so
 * it is validated here before anything renders it: unknown fields are dropped, every list and string
 * is bounded, and images must be small JPEG/PNG/WebP `data:` URLs. Only the server and the upload
 * preview parse reports; pages that show one import report.ts, so zod stays out of their bundles.
 */
import type { JsonValue } from '@atproto/lex-json';
import { z } from 'zod';
import { MIX_REPORT_FORMAT, MIX_REPORT_MAX_BYTES, MIX_REPORT_VERSION, type MixReport } from './report.ts';

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

export const trackSchema = z.object({
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
	/** The slowest and fastest speeds, when the pitch fader was ridden. */
	speedRange: z.tuple([z.number().min(0.25).max(4), z.number().min(0.25).max(4)]).optional(),
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
