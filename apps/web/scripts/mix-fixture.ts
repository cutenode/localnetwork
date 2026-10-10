/**
 * Regenerates src/lib/mix/fixtures/v2.mix.json with locallibrary's real writer, so the parser is
 * tested against the format as locallibrary writes it. The fixture is `locallibrary mix … --export`
 * run on a two-minute mix of rekordcrate's two demo tracks (from locallibrary's test fixtures).
 * The demo export ships without audio, so the tracks are synthesized, and everything happens in a
 * temporary folder with its own store: no library or drive of yours is read or written.
 *
 *   pnpm fixtures:mix
 *
 * Builds locallibrary from this repo's Cargo workspace, so it needs cargo.
 */
import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const RATE = 22_050;

// The repo root, which holds the Cargo workspace (pnpm runs this from apps/web).
const repo = resolve('../..');

const fixture = resolve('src/lib/mix/fixtures/v2.mix.json');

/** Random notes (four a second, two at a time) with a little noise, so fingerprints have something to hold. */
function melody(seed: number, secs: number): Float32Array {
	let state = seed;

	const random = () => {
		state = (Math.imul(state, 1_103_515_245) + 12_345) >>> 0;

		return state / 2 ** 32;
	};

	const note = () => 110 * 2 ** (Math.floor(random() * 36) / 12);
	const out = new Float32Array(secs * RATE);
	const length = RATE / 4;

	for (let start = 0; start + length <= out.length; start += length) {
		const [f, g] = [note(), note()];

		for (let i = 0; i < length; i++) {
			const t = i / RATE;
			out[start + i] =
				0.4 * Math.sin(2 * Math.PI * f * t) + 0.3 * Math.sin(2 * Math.PI * g * t) + 0.05 * (random() - 0.5);
		}
	}

	return out;
}

/** 16-bit mono PCM WAV. */
function wav(samples: Float32Array): Buffer {
	const out = Buffer.alloc(44 + samples.length * 2);
	out.write('RIFF', 0);
	out.writeUInt32LE(36 + samples.length * 2, 4);
	out.write('WAVEfmt ', 8);
	out.writeUInt32LE(16, 16);
	out.writeUInt16LE(1, 20);
	out.writeUInt16LE(1, 22);
	out.writeUInt32LE(RATE, 24);
	out.writeUInt32LE(RATE * 2, 28);
	out.writeUInt16LE(2, 32);
	out.writeUInt16LE(16, 34);
	out.write('data', 36);
	out.writeUInt32LE(samples.length * 2, 40);
	samples.forEach((s, i) => out.writeInt16LE(Math.round(Math.max(-1, Math.min(1, s)) * 30_000), 44 + i * 2));

	return out;
}

const dir = mkdtempSync(join(tmpdir(), 'mix-fixture-'));

try {
	const drive = join(dir, 'Main');
	cpSync(join(repo, 'crates/locallibrary/tests/fixtures/rekordcrate/demo_tracks'), drive, { recursive: true });
	const [one, two] = [melody(1, 90), melody(2, 90)];
	const folder = join(drive, 'Contents/Loopmasters/UnknownAlbum');
	mkdirSync(folder, { recursive: true });
	// The demo export names MP3s; locallibrary goes by what a file holds, so WAVs there decode.
	writeFileSync(join(folder, 'Demo Track 1.mp3'), wav(one));
	writeFileSync(join(folder, 'Demo Track 2.mp3'), wav(two));

	// A minute of each: from 10s into the first, then from 5s into the second.
	const set = new Float32Array(120 * RATE);
	set.set(one.subarray(10 * RATE, 70 * RATE));
	set.set(two.subarray(5 * RATE, 65 * RATE), 60 * RATE);
	writeFileSync(join(dir, 'friday.wav'), wav(set));

	const out = join(dir, 'out');
	const args = ['mix', join(dir, 'friday.wav'), '--drive', drive, '--export', `${out}/`];
	execFileSync(
		'cargo',
		['run', '--locked', '-q', '--manifest-path', join(repo, 'Cargo.toml'), '-p', 'locallibrary-cli', '--', ...args],
		{
			// Its own store, and no drives looked for beyond the one named.
			env: { ...process.env, LOCALLIBRARY_STORE: join(dir, 'store'), LOCALLIBRARY_VOLUMES: '' }
		}
	);
	cpSync(join(out, readdirSync(out)[0]), fixture);
	execFileSync('pnpm', ['exec', 'oxfmt', fixture]);
} finally {
	rmSync(dir, { recursive: true, force: true });
}
