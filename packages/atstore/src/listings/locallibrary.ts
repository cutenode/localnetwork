// locallibrary's captures: the real player in a terminal (recorded by VHS, which drives it in
// ttyd and headless Chrome), wide and phone-sized, on fixture drives with synthetic audio (each on
// a disk image mounted as a USB stick, sticks.ts), the HTML report of a set recorded from them, and
// rekordbox's analysis of a demo track for the website's waveform. Nothing it runs sees the
// machine's drives or store, and nothing plays aloud.
import { execFileSync } from 'node:child_process';
import { copyFileSync, cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import type { Browser } from 'playwright';
import { ROOT, run } from '../run.ts';
import { mountSticks } from '../sticks.ts';
import type { Capture, Listing } from '../types.ts';

const BOMBADIL = join(ROOT, 'apps/locallibrary/bombadil');

/** The player's dark palette (loclib-waveform's Palette::DARK) as the terminal's colours, on the icon's background. */
const THEME = {
	background: '#101016',
	foreground: '#e2e2ec',
	cursor: '#101016',
	selection: '#2c2c46',
	black: '#101016',
	red: '#ff5c7a',
	green: '#5cd68a',
	yellow: '#ffc456',
	blue: '#408cff',
	magenta: '#ff5ca0',
	cyan: '#5cd6ff',
	white: '#e2e2ec',
	brightBlack: '#48485c',
	brightRed: '#ff8099',
	brightGreen: '#80e6a6',
	brightYellow: '#ffd480',
	brightBlue: '#73a9ff',
	brightMagenta: '#ff85b8',
	brightCyan: '#85e2ff',
	brightWhite: '#ffffff'
};

/**
 * The terminals the tapes are recorded in, in pixels. The phone's comes to 65 columns by 67 rows,
 * for showing 390 CSS pixels wide: any narrower and the library's track list cuts titles to a few
 * letters, then drops them.
 */
const TERMINALS = {
	wide: { suffix: '', width: 2560, height: 1536, fontSize: 29, padding: 40 },
	phone: { suffix: '-phone', width: 780, height: 1400, fontSize: 17, padding: 20 }
};

type Terminal = (typeof TERMINALS)[keyof typeof TERMINALS];

/** The windows the set's report is shown in: a laptop's and a phone's. */
const VIEWPORTS = [
	{ suffix: '', viewport: { width: 1440, height: 900 } },
	{ suffix: '-phone', viewport: { width: 390, height: 844 } }
];

/** The demo track whose analysis the website draws its waveform from. */
const DEMO_TRACK = 'Demo Track 1';

interface Track {
	title: string;
	file_path: string;
}

/** Fills `drive` with a WAV for each of its tracks, and with `set`, a recorded set of four of them. */
function synthesize(bin: string, env: Record<string, string>, drive: string, set?: string): Track[] {
	const tracks = execFileSync(bin, ['--json', 'tracks', drive], { env: { ...process.env, ...env } });
	execFileSync('node', [join(BOMBADIL, 'make-mix.mjs'), drive, ...(set ? [set] : [])], {
		input: tracks,
		stdio: ['pipe', 'ignore', 'inherit']
	});
	const listed: { tracks: Track[] } = JSON.parse(tracks.toString());

	return listed.tracks;
}

/**
 * rekordbox's analysis of `track` (its overview waveform, phrases, cues, key and tempo), as a mix
 * report's play carries it: the track's own audio, identified as a recording, plays just it.
 */
async function analysis(bin: string, env: Record<string, string>, drive: string, track: Track, work: string) {
	const recording = join(work, 'demo.wav');
	const data = join(work, 'demo.mix.json');
	copyFileSync(join(drive, track.file_path), recording);
	await run(bin, ['--quiet', 'mix', '--drive', drive, '--export', data, recording], { env });
	const { plays }: { plays: { track: { title: string } }[] } = JSON.parse(readFileSync(data, 'utf8'));
	const play = plays.find((p) => p.track.title === track.title);

	if (!play) throw new Error(`the recording of ${track.title} wasn't identified as it`);

	return Buffer.from(JSON.stringify(play.track));
}

/**
 * A tape for `terminal`: on DJ (whose recording folder holds the set), the library, the Drives
 * view with DJ picked, the snapshots and the set in the Mixes view; DJ checked from the shell; then,
 * on DEMO, a track playing full screen. DEMO is rekordbox's demo export, whose two tracks come
 * with their waveforms, beat grid and phrases. A screenshot is of the frame after it, so each is
 * followed by a pause.
 */
function tape(bin: string, env: Record<string, string>, volumes: string, terminal: Terminal): string {
	const open = (drive: string) =>
		`Type ${JSON.stringify(`clear; locallibrary tui ${join(volumes, drive)}`)}\nEnter\nSleep 5s`;

	const shot = (name: string) => `Show\nSleep 1s\nScreenshot "${name}${terminal.suffix}.png"\nSleep 1s\nHide`;

	return [
		'Output "tape.gif"',
		'Set Shell "bash"',
		`Set FontSize ${terminal.fontSize}`,
		`Set Width ${terminal.width}`,
		`Set Height ${terminal.height}`,
		`Set Padding ${terminal.padding}`,
		`Set Theme ${JSON.stringify(THEME)}`,
		...Object.entries(env).map(([key, value]) => `Env ${key} ${JSON.stringify(value)}`),
		'Hide',
		// The command as it's typed, without the warning that LOCALLIBRARY_VOLUMES is set.
		`Type \`locallibrary() { '${bin}' "$@" 2>/dev/null; }\`\nEnter`,
		open('DJ'),
		shot('library'),
		// Down to the last row, DJ's drive, whichever row the view starts on.
		'Type "2"\nSleep 2s\nType "jjjj"\nSleep 1s',
		shot('drives'),
		'Type "3"\nSleep 2s',
		shot('snapshots'),
		'Type "5"\nSleep 2s\nEnter\nSleep 10s',
		shot('mixes'),
		'Ctrl+C\nSleep 2s\nType "clear"\nEnter\nSleep 1s',
		'Type "locallibrary check DJ --players CDJ-3000,CDJ-2000NXS2"\nEnter\nSleep 5s',
		shot('check'),
		open('DEMO'),
		'Type "1"\nSleep 1s\nEnter\nSleep 6s\nType "f"\nSleep 3s',
		shot('player'),
		// Quit, or the player outlives the recording and keeps the stick from ejecting.
		'Ctrl+C\nSleep 2s'
	].join('\n');
}

/** The set's report, as a browser shows it wide and on a phone. */
async function report(browser: Browser, html: string): Promise<Capture[]> {
	const captures: Capture[] = [];

	for (const { suffix, viewport } of VIEWPORTS) {
		const page = await browser.newPage({ viewport, deviceScaleFactor: 2, colorScheme: 'dark' });
		await page.goto(pathToFileURL(html).href, { waitUntil: 'load' });
		captures.push({ file: `report${suffix}.png`, bytes: await page.screenshot({ type: 'png' }) });
		await page.close();
	}

	return captures;
}

/** Everything there is to capture, made on the drives mounted in `volumes`, with the store and the rest in `work`. */
async function record(browser: Browser, bin: string, work: string, volumes: string): Promise<Capture[]> {
	const env = {
		LOCALLIBRARY_VOLUMES: volumes,
		LOCALLIBRARY_STORE: join(work, 'store'),
		LOCALLIBRARY_AUDIO: 'silent',
		LOCALLIBRARY_REDUCED_MOTION: '1',
		COLORTERM: 'truecolor',
		HOME: work
	};

	const [dj, demo] = [join(volumes, 'DJ'), join(volumes, 'DEMO')];
	const set = join(dj, 'PIONEER REC/REC001.WAV');

	await run('bash', ['-c', '. "$0/lib.sh" && fixture_drive "$1"', BOMBADIL, dj]);
	cpSync(join(ROOT, 'crates/locallibrary/tests/fixtures/rekordcrate/demo_tracks/PIONEER'), join(demo, 'PIONEER'), {
		recursive: true
	});
	synthesize(bin, env, dj, set);
	const track = synthesize(bin, env, demo).find((t) => t.title === DEMO_TRACK);

	if (!track) throw new Error(`DEMO has no ${DEMO_TRACK}`);
	const demoTrack = await analysis(bin, env, demo, track, work);
	await run(bin, ['--quiet', 'mix', '--drive', dj, '--html', join(work, 'set.html'), set], { env });
	// Snapshots in the store, for the Snapshots view and the Drives view's store status.
	await run(bin, ['--quiet', 'pull', dj, '-m', "after Friday's export"], { env });
	await run(bin, ['--quiet', 'pull', demo], { env });

	const names = ['player', 'library', 'mixes', 'drives', 'snapshots', 'check'];
	const terminal: Capture[] = [];

	for (const t of Object.values(TERMINALS)) {
		writeFileSync(join(work, 'shots.tape'), tape(bin, env, volumes, t));
		await run('vhs', ['shots.tape'], { cwd: work });

		for (const name of names) {
			const file = `${name}${t.suffix}.png`;
			terminal.push({ file, bytes: readFileSync(join(work, file)) });
		}
	}

	return [
		...terminal,
		...(await report(browser, join(work, 'set.html'))),
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
	async capture(browser) {
		await run('cargo', ['build', '--quiet', '--locked', '-p', 'locallibrary-cli']);
		const bin = join(process.env.CARGO_TARGET_DIR ?? join(ROOT, 'target'), 'debug/locallibrary');
		// Short, as the Drives view shows the drives' paths: /private/tmp/ll-AbC123/Volumes/DJ.
		const work = mkdtempSync('/tmp/ll-');

		try {
			const sticks = await mountSticks(work, ['DJ', 'DEMO']);

			try {
				return await record(browser, bin, work, sticks.volumes);
			} finally {
				await sticks.eject();
			}
		} finally {
			rmSync(work, { recursive: true, force: true });
		}
	}
};
