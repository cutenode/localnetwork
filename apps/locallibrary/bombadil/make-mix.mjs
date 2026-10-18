// Synthetic audio for the mix report test: a WAV for every track on a drive (read from
// `locallibrary --json tracks` on stdin), and, given a path for it, a "recorded set" that plays
// some of them in turn with crossfades, so `locallibrary mix` has something real to identify.
//
//   locallibrary --json tracks DRIVE | node make-mix.mjs DRIVE [MIX.wav]

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

const RATE = 22050;
const TRACK_SECS = 75;
const PLAYED = [0, 3, 1, 5];
const SEGMENT_SECS = 40;
const FADE_SECS = 8;

const [drive, mixPath] = process.argv.slice(2);
if (!drive) {
  console.error("usage: locallibrary --json tracks DRIVE | node make-mix.mjs DRIVE [MIX.wav]");
  process.exit(2);
}
const { tracks } = JSON.parse(readFileSync(0, "utf8"));

/** A deterministic, noise-like signal with structure (decaying two-partial notes). */
function synth(secs, seed) {
  const n = Math.floor(secs * RATE);
  const out = new Float32Array(n);
  let state = (Math.imul(seed, 2654435761) >>> 0) || 1;
  const rand = () => {
    state ^= state << 13;
    state >>>= 0;
    state ^= state >>> 17;
    state ^= state << 5;
    state >>>= 0;
    return state / 0xffffffff;
  };
  const note = Math.floor(0.12 * RATE);
  for (let start = 0; start < n; start += note) {
    const f = 150 + rand() * 2500;
    const g = 0.15 + rand() * 0.2;
    for (let i = 0; i < Math.min(note * 3, n - start); i++) {
      const t = i / RATE;
      const env = Math.exp(-t * 12) * g;
      out[start + i] += env * (Math.sin(2 * Math.PI * f * t) + 0.5 * Math.sin(2 * Math.PI * f * 1.5 * t));
    }
  }
  return out;
}

function wav(samples) {
  const buf = Buffer.alloc(44 + samples.length * 2);
  buf.write("RIFF", 0);
  buf.writeUInt32LE(36 + samples.length * 2, 4);
  buf.write("WAVEfmt ", 8);
  buf.writeUInt32LE(16, 16);
  buf.writeUInt16LE(1, 20); // PCM
  buf.writeUInt16LE(1, 22); // mono
  buf.writeUInt32LE(RATE, 24);
  buf.writeUInt32LE(RATE * 2, 28);
  buf.writeUInt16LE(2, 32);
  buf.writeUInt16LE(16, 34);
  buf.write("data", 36);
  buf.writeUInt32LE(samples.length * 2, 40);
  samples.forEach((s, i) => buf.writeInt16LE(Math.round(Math.max(-1, Math.min(1, s)) * 32767), 44 + i * 2));
  return buf;
}

// Each track's file, at the path the database expects (WAV data whatever the extension).
const audio = tracks.map((t, i) => {
  const samples = synth(TRACK_SECS, 7919 * (i + 1));
  const path = join(drive, t.file_path);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, wav(samples));
  return samples;
});
if (!mixPath) {
  console.error(`wrote ${tracks.length} tracks`);
  process.exit(0);
}

// The set: a stretch from the middle of each played track, crossfaded into the next.
const seg = SEGMENT_SECS * RATE;
const fade = FADE_SECS * RATE;
const mix = new Float32Array(PLAYED.length * (seg - fade) + fade);
PLAYED.forEach((which, k) => {
  const from = 10 * RATE;
  const at = k * (seg - fade);
  for (let i = 0; i < seg; i++) {
    const gain = Math.min(1, i / fade, (seg - i) / fade);
    mix[at + i] += audio[which][from + i] * (k === 0 && i < fade ? 1 : gain);
  }
});
mkdirSync(dirname(mixPath), { recursive: true });
writeFileSync(mixPath, wav(mix));
console.error(`wrote ${tracks.length} tracks and a ${Math.round(mix.length / RATE)} s mix playing tracks ${PLAYED.map((i) => tracks[i].id).join(", ")}`);
