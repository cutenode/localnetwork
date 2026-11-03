// locallibrary's captures: the real player in a terminal (recorded by VHS, which drives it in
// ttyd and headless Chrome, from locallibrary-tape.ts), wide and phone-sized, on fixture drives
// with synthetic audio (each on a disk image mounted as a USB stick, sticks.ts), the HTML report of
// a set recorded from them, and rekordbox's analysis of a demo track for the website's waveform.
// Nothing it runs sees the machine's drives or store, and nothing plays aloud.
import { execFileSync } from 'node:child_process';
import { copyFileSync, cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import type { Browser } from 'playwright';
import { lock } from '../lock.ts';
import { ROOT, run } from '../run.ts';
import { ejectLeftovers, mountSticks } from '../sticks.ts';
import type { Capture, Listing } from '../types.ts';
import { killPlayers, tape, TERMINALS } from './locallibrary-tape.ts';

const BOMBADIL = join(ROOT, 'apps/locallibrary/bombadil');

/**
 * The run's own folder: fixed, so the paths the Drives view shows are the same every run, and
 * short, as it shows them whole: /private/tmp/atstore/Volumes/DJ. A lock beside it keeps a second
 * run (from another checkout, say) out of it.
 */
const WORK = '/tmp/atstore';

/**
 * The windows the set's report is shown in, at the website's ratios, drawn 2560 by 1600 and 780 by
 * 1400: a small laptop's, so its text reads at the 750 CSS pixels the website shows it at, and a
 * phone's.
 */
const VIEWPORTS = [
	{ suffix: '', viewport: { width: 1024, height: 640 }, deviceScaleFactor: 2.5 },
	{ suffix: '-phone', viewport: { width: 390, height: 700 }, deviceScaleFactor: 2 }
];

/** The demo track whose analysis the website draws its waveform from. */
const DEMO_TRACK = 'Demo Track 1';

/** The DJ track upgraded to a lossless file no player in the tape's check can load. */
const HI_RES_TRACK = 'On Yo Back';

interface Track {
	title: string;
	file_path: string;
}

type Env = Record<string, string>;

/** Fills `drive` with a WAV for each of its tracks, and with `set`, a recorded set of four of them. */
function synthesize(bin: string, env: Env, drive: string, set?: string): Track[] {
	const tracks = execFileSync(bin, ['--json', 'tracks', drive], { env: { ...process.env, ...env } });
	execFileSync('node', [join(BOMBADIL, 'make-mix.mjs'), drive, ...(set ? [set] : [])], {
		input: tracks,
		stdio: ['pipe', 'ignore', 'inherit']
	});
	const listed: { tracks: Track[] } = JSON.parse(tracks.toString());

	return listed.tracks;
}

/**
 * A 24-bit, 176.4 kHz WAV of `wav` (a 16-bit mono WAV at 22.05 kHz, as make-mix.mjs writes),
 * each sample held for eight: the same recording as far as `track upgrade` can hear, at a rate
 * neither the CDJ-3000 nor the CDJ-2000NXS2 plays.
 */
function hiRes(wav: Buffer): Buffer {
	const times = 8;
	const rate = wav.readUInt32LE(24) * times;
	const samples = (wav.length - 44) / 2;
	const out = Buffer.alloc(44 + samples * times * 3);
	out.write('RIFF', 0);
	out.writeUInt32LE(36 + samples * times * 3, 4);
	out.write('WAVEfmt ', 8);
	out.writeUInt32LE(16, 16);
	out.writeUInt16LE(1, 20); // PCM
	out.writeUInt16LE(1, 22); // mono
	out.writeUInt32LE(rate, 24);
	out.writeUInt32LE(rate * 3, 28);
	out.writeUInt16LE(3, 32);
	out.writeUInt16LE(24, 34);
	out.write('data', 36);
	out.writeUInt32LE(samples * times * 3, 40);

	for (let i = 0; i < samples; i++) {
		const sample = wav.readInt16LE(44 + i * 2) * 256;

		for (let k = 0; k < times; k++) out.writeIntLE(sample, 44 + (i * times + k) * 3, 3);
	}

	return out;
}

/**
 * rekordbox's analysis of `track` (its overview waveform, phrases, cues, key and tempo), as a mix
 * report's play carries it: the track's own audio, identified as a recording, plays just it.
 */
async function analysis(bin: string, env: Env, drive: string, track: Track, signal: AbortSignal) {
	const recording = join(WORK, 'demo.wav');
	const data = join(WORK, 'demo.mix.json');
	copyFileSync(join(drive, track.file_path), recording);
	await run(bin, ['--quiet', 'mix', '--drive', drive, '--export', data, recording], { env, signal });
	const { plays }: { plays: { track: { title: string } }[] } = JSON.parse(readFileSync(data, 'utf8'));
	const play = plays.find((p) => p.track.title === track.title);

	if (!play) throw new Error(`the recording of ${track.title} wasn't identified as it`);

	return Buffer.from(JSON.stringify(play.track));
}

/** The set's report, as a browser shows it wide and on a phone, scrolled to the mix's waveform. */
async function report(browser: Browser, html: string): Promise<Capture[]> {
	const captures: Capture[] = [];

	for (const { suffix, ...options } of VIEWPORTS) {
		const page = await browser.newPage({ ...options, colorScheme: 'dark' });
		await page.goto(pathToFileURL(html).href, { waitUntil: 'load' });
		await page.locator('#deck').evaluate((deck) => window.scrollBy(0, deck.getBoundingClientRect().top - 16));
		captures.push({ file: `report${suffix}.png`, bytes: await page.screenshot({ type: 'png' }) });
		await page.close();
	}

	return captures;
}

/** Everything there is to capture, made on the drives mounted in `volumes`, with the store and the rest in WORK. */
async function record(browser: Browser, bin: string, volumes: string, signal: AbortSignal): Promise<Capture[]> {
	const env = {
		LOCALLIBRARY_VOLUMES: volumes,
		LOCALLIBRARY_STORE: join(WORK, 'store'),
		LOCALLIBRARY_AUDIO: 'silent',
		LOCALLIBRARY_REDUCED_MOTION: '1',
		COLORTERM: 'truecolor',
		// `check` as it prints, without less: on a phone-sized terminal less leaves its "Waiting
		// for data..." line under the output.
		PAGER: 'cat',
		HOME: WORK
	};

	const locallibrary = (...args: string[]) => run(bin, ['--quiet', '--yes', ...args], { env, signal });
	const [dj, demo] = [join(volumes, 'DJ'), join(volumes, 'DEMO')];
	const set = join(dj, 'PIONEER REC/REC001.WAV');

	await run('bash', ['-c', '. "$0/lib.sh" && fixture_drive "$1"', BOMBADIL, dj], { signal });
	cpSync(join(ROOT, 'crates/locallibrary/tests/fixtures/rekordcrate/demo_tracks/PIONEER'), join(demo, 'PIONEER'), {
		recursive: true
	});
	const djTracks = synthesize(bin, env, dj, set);
	const track = synthesize(bin, env, demo).find((t) => t.title === DEMO_TRACK);

	if (!track) throw new Error(`DEMO has no ${DEMO_TRACK}`);
	const demoTrack = await analysis(bin, env, demo, track, signal);
	await locallibrary('mix', '--drive', dj, '--html', join(WORK, 'set.html'), set);
	// Snapshots in the store, for the Snapshots view and the Drives view's store status: DJ's as
	// exported, and again once a track is upgraded to a file `check` finds the players can't load.
	await locallibrary('pull', dj, '-m', "Friday's export");
	const hiResTrack = djTracks.find((t) => t.title === HI_RES_TRACK);

	if (!hiResTrack) throw new Error(`DJ has no ${HI_RES_TRACK}`);
	const lossless = join(WORK, 'lossless');
	mkdirSync(lossless);
	writeFileSync(
		join(lossless, basename(hiResTrack.file_path).replace(/\.\w+$/, '.wav')),
		hiRes(readFileSync(join(dj, hiResTrack.file_path)))
	);
	await locallibrary('track', 'upgrade', dj, '--from', lossless);
	await locallibrary('pull', dj, '-m', 'hi-res upgrade');
	await locallibrary('pull', demo);

	const terminal: Capture[] = [];

	for (const t of TERMINALS) {
		writeFileSync(join(WORK, 'shots.tape'), tape(bin, env, volumes, t));

		try {
			await run('vhs', ['shots.tape'], { cwd: WORK, signal });
		} finally {
			killPlayers(WORK);
		}

		for (const view of t.views) {
			const file = `${view}${t.suffix}.png`;
			terminal.push({ file, bytes: readFileSync(join(WORK, file)) });
		}
	}

	return [
		...terminal,
		...(await report(browser, join(WORK, 'set.html'))),
		{ file: 'demo-track.json', bytes: demoTrack }
	];
}

export const locallibrary: Listing = {
	id: 'locallibrary',
	name: 'locallibrary',
	eyebrow: 'Local Network · for rekordbox USBs',
	icon: 'apps/locallibrary/assets/icon.svg',
	glow: '#ff5ca0',
	hero: { shots: ['player'], frame: { kind: 'terminal', title: 'locallibrary' } },
	screenshots: ['player', 'library', 'mixes', 'report'],
	page: true,
	async capture(browser, signal) {
		await run('cargo', ['build', '--quiet', '--locked', '-p', 'locallibrary-cli'], { signal });
		const bin = join(process.env.CARGO_TARGET_DIR ?? join(ROOT, 'target'), 'debug/locallibrary');
		const unlock = lock(`${WORK}.lock`);

		try {
			// A run killed before it could clean up leaves its sticks mounted in WORK.
			if (!(await ejectLeftovers(WORK))) throw new Error(`sticks in ${WORK} are still mounted`);
			rmSync(WORK, { recursive: true, force: true });
			mkdirSync(WORK);
			const sticks = await mountSticks(WORK, ['DJ', 'DEMO']);

			try {
				return await record(browser, bin, sticks.volumes, signal);
			} finally {
				const ejected = await sticks.eject();

				// Removing WORK with a stick still mounted in it would empty the stick first, then fail.
				if (ejected) rmSync(WORK, { recursive: true, force: true });
				else console.error(`left ${WORK} in place, as a stick in it wouldn't eject`);
			}
		} finally {
			unlock();
		}
	}
};
