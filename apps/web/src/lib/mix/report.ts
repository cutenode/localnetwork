/**
 * Mix reports: what a DJ's recorded set plays, when, and how, as exported by locallibrary
 * (`locallibrary mix <set> --export set.mix.json`), and the helpers that show one. schema.ts
 * validates the file.
 *
 * Format `locallibrary.mix-report`, versions 1 and 2. Version 2 dropped `library` (the drive's name
 * and track count), so a version 1 report's is dropped here too. Times are seconds into the mix,
 * except a play's `trackStart`/`trackEnd` and a track's cues and phrases, which are seconds into
 * the track.
 */
import type { z } from 'zod';
import type { mixReportSchema } from './schema.ts';
import colors from '../../../../../packages/tokens/colors.json' with { type: 'json' };
import terms from '../../../../../packages/tokens/mix.json' with { type: 'json' };

/** Colours that mean the same here as in locallibrary's player and HTML report. */
export { colors };

/** How long two tracks are heard together, in seconds, before the transition is a blend, as locallibrary and published sets have it. */
export const BLEND_SECS = terms.blendSecs;

export const MIX_REPORT_FORMAT = 'locallibrary.mix-report';

/** The newest format version this site reads. */
export const MIX_REPORT_VERSION = 2;

/** Largest export accepted, as uploaded. */
export const MIX_REPORT_MAX_BYTES = 1_000_000;

export type MixReport = z.infer<typeof mixReportSchema>;

export type MixPlay = MixReport['plays'][number];

export type MixTrack = MixPlay['track'];

export type MixTransition = MixReport['transitions'][number];

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
		t.overlap >= BLEND_SECS
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

/** Each role's background, spelled out so Tailwind generates them. The light palette has its own. */
const phraseBackgrounds: Record<PhraseRole, string> = {
	intro: 'bg-phrase-intro',
	up: 'bg-phrase-up',
	verse: 'bg-phrase-verse',
	chorus: 'bg-phrase-chorus',
	down: 'bg-phrase-down',
	bridge: 'bg-phrase-bridge',
	outro: 'bg-phrase-outro',
	unknown: 'bg-phrase-unknown'
};

/** A phrase's colour as a background class, by its role, as the player and locallibrary's report draw it. */
export function phraseBackground(kind: string): string {
	return phraseBackgrounds[phraseRole(kind)];
}

/** Track colours for plays in turn, leaving out red and green, which mean a clash and a match. */
const playPalette = ['blue', 'aqua', 'yellow', 'orange', 'pink', 'purple'] as const;

/** The colour a play is drawn in across the timeline, tracklist and key wheel. */
export function playColor(index: number): string {
	return colors.trackColors[playPalette[index % playPalette.length]];
}

/** The keys the mix moves through as played, with repeats in a row merged. */
export function keyPath(report: MixReport): string[] {
	const path: string[] = [];

	for (const { playedKey } of report.plays) if (playedKey && playedKey !== path.at(-1)) path.push(playedKey);

	return path;
}
