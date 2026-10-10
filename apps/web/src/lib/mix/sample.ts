/**
 * A made-up mix report, for tests and for trying the viewer without an export. The tracks,
 * artists and labels are invented.
 */
import { MIX_REPORT_FORMAT, MIX_REPORT_VERSION, type MixReport } from './report';

const wave = (n: number, seed: number, base: number, swing: number) =>
	Array.from({ length: n }, (_, i) =>
		Math.round(base + swing * Math.abs(Math.sin(i / 7 + seed) * Math.cos(i / 31 + seed)))
	);

const overview = (seed: number) => ({
	heights: wave(200, seed, 60, 180),
	colors: Array.from({ length: 200 }, (_, i) =>
		Math.sin(i / 9 + seed) > 0.3 ? '#2050ff' : i % 5 ? '#ff7a1a' : '#ffffff'
	)
});

const plays = [
	['Night Bus', 'Ferrule', 'Cobble Records', 124, '8A', 0, 330],
	['Signal Path', 'Mira Okonkwo', 'Floor Two', 125, '8A', 300, 640],
	['Halogen', 'Two Rooms', 'Cobble Records', 126, '9A', 610, 980],
	['Low Tide (Dub)', 'Ferrule, Ava Lind', 'Saltmarsh', 124, '10A', 1020, 1380],
	['Static Bloom', 'Kestrel Unit', 'Floor Two', 128, '3B', 1360, 1720]
] as const;

export const sampleReport: MixReport = {
	format: MIX_REPORT_FORMAT,
	version: MIX_REPORT_VERSION,
	generator: { name: 'locallibrary', version: '0.1.0' },
	analysedAt: '2026-10-05T08:16:00Z',
	mix: {
		title: 'Sunday Warm-up',
		artist: 'Sample DJ',
		genre: 'House',
		date: '2026-10-04',
		recorder: 'XDJ-AZ',
		durationSecs: 1740
	},
	tempoRange: 0.16,
	coverage: 0.93,
	waveform: {
		secondsPerColumn: 1.45,
		low: wave(1200, 1, 40, 200),
		mid: wave(1200, 2, 30, 160),
		high: wave(1200, 3, 20, 120)
	},
	plays: plays.map(([title, artist, label, bpm, key, start, end], i) => ({
		track: {
			title,
			artist,
			label,
			bpm,
			key,
			durationSecs: 400,
			overview: overview(i),
			cues: [
				{ at: 16, label: 'A', color: '#28e214' },
				{ at: 96, label: 'B', color: '#e81414' }
			],
			phrases: [
				{ start: 0, end: 60, kind: 'Intro' },
				{ start: 60, end: 300, kind: 'Up' },
				{ start: 300, end: 400, kind: 'Outro' }
			]
		},
		start,
		end,
		trackStart: 10,
		trackEnd: 10 + (end - start),
		tempo: 126 / bpm,
		playedBpm: 126,
		keyLock: i % 2 === 0,
		semitones: i % 2 === 0 ? 0 : 0.3,
		key,
		playedKey: key,
		confidence: i === 4 ? 0.42 : 0.9,
		presence: wave(Math.ceil((end - start) / 2), i, 20, 80).map((v) => Math.min(100, v)),
		presenceStep: 2,
		jumps: i === 2 ? 1 : 0
	})),
	transitions: [
		{ from: 0, to: 1, at: 300, overlap: 30, harmony: 'same key', tempoChange: 0.8 },
		{ from: 1, to: 2, at: 610, overlap: 30, harmony: '±1' },
		{ from: 2, to: 3, at: 1020, overlap: -40, harmony: '±1', tempoChange: -1.6 },
		{ from: 3, to: 4, at: 1360, overlap: 20, harmony: 'clash', tempoChange: 3.2 }
	],
	unidentified: [{ start: 980, end: 1020 }]
};
