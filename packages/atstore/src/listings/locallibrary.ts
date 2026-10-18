// locallibrary's screenshots: the real player in a terminal (recorded by VHS, which drives it in
// ttyd and headless Chrome), on fixture drives with synthetic audio, and the HTML report of a set
// recorded from them. Nothing it runs sees the machine's drives or store, and nothing plays aloud.
import { execFileSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { ROOT, run } from '../run.ts';
import type { Listing, Shot } from '../types.ts';

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

/** Fills `drive` with a WAV for each of its tracks, and with `set`, a recorded set of four of them. */
function synthesize(bin: string, env: Record<string, string>, drive: string, set?: string) {
	const tracks = execFileSync(bin, ['--json', 'tracks', drive], { env: { ...process.env, ...env } });
	execFileSync('node', [join(BOMBADIL, 'make-mix.mjs'), drive, ...(set ? [set] : [])], {
		input: tracks,
		stdio: ['pipe', 'ignore', 'inherit']
	});
}

/**
 * The tape: the library and the set in the Mixes view on DJ (whose recording folder holds the
 * set), then, on DEMO, a track playing full screen. DEMO is rekordbox's demo export, whose two
 * tracks come with their waveforms, beat grid and phrases. A screenshot is of the frame after it,
 * so each is followed by a pause.
 */
function tape(bin: string, env: Record<string, string>, volumes: string): string {
	const open = (drive: string) =>
		`Type ${JSON.stringify(`clear; ${bin} tui ${join(volumes, drive)} 2>/dev/null`)}\nEnter\nSleep 5s`;

	const shot = (name: string) => `Show\nSleep 1s\nScreenshot "${name}.png"\nSleep 1s\nHide`;

	return [
		'Output "tape.gif"',
		'Set Shell "bash"',
		'Set FontSize 29',
		'Set Width 2560',
		'Set Height 1536',
		'Set Padding 40',
		`Set Theme ${JSON.stringify(THEME)}`,
		...Object.entries(env).map(([key, value]) => `Env ${key} ${JSON.stringify(value)}`),
		'Hide',
		open('DJ'),
		shot('library'),
		'Type "5"\nSleep 2s\nEnter\nSleep 10s',
		shot('mixes'),
		'Ctrl+C\nSleep 2s',
		open('DEMO'),
		'Type "1"\nSleep 1s\nEnter\nSleep 6s\nType "f"\nSleep 3s',
		shot('player')
	].join('\n');
}

export const locallibrary: Listing = {
	id: 'locallibrary',
	name: 'locallibrary',
	eyebrow: 'Local Network · for rekordbox USBs',
	icon: 'apps/locallibrary/assets/icon.svg',
	glow: '#ff5ca0',
	hero: { shots: [0], frame: { kind: 'terminal', title: 'locallibrary' } },
	async capture(browser) {
		await run('cargo', ['build', '--quiet', '--locked', '-p', 'locallibrary-cli']);
		const bin = join(process.env.CARGO_TARGET_DIR ?? join(ROOT, 'target'), 'debug/locallibrary');
		const work = mkdtempSync(join(tmpdir(), 'locallibrary-atstore-'));

		try {
			const volumes = join(work, 'volumes');

			const env = {
				LOCALLIBRARY_VOLUMES: volumes,
				LOCALLIBRARY_STORE: join(work, 'store'),
				LOCALLIBRARY_AUDIO: 'silent',
				LOCALLIBRARY_REDUCED_MOTION: '1',
				COLORTERM: 'truecolor',
				HOME: work
			};

			const set = join(volumes, 'DJ/PIONEER REC/REC001.WAV');

			await run('bash', ['-c', '. "$0/lib.sh" && fixture_drive "$1"', BOMBADIL, join(volumes, 'DJ')]);
			cpSync(
				join(ROOT, 'crates/locallibrary/tests/fixtures/rekordcrate/demo_tracks/PIONEER'),
				join(volumes, 'DEMO/PIONEER'),
				{ recursive: true }
			);
			synthesize(bin, env, join(volumes, 'DJ'), set);
			synthesize(bin, env, join(volumes, 'DEMO'));
			await run(bin, ['--quiet', 'mix', '--drive', join(volumes, 'DJ'), '--html', join(work, 'set.html'), set], {
				env
			});

			writeFileSync(join(work, 'shots.tape'), tape(bin, env, volumes));
			await run('vhs', ['shots.tape'], { cwd: work });
			const terminal = (name: string): Shot => ({ name, png: readFileSync(join(work, `${name}.png`)) });

			const page = await browser.newPage({
				viewport: { width: 1440, height: 900 },
				deviceScaleFactor: 2,
				colorScheme: 'dark'
			});

			await page.goto(pathToFileURL(join(work, 'set.html')).href, { waitUntil: 'load' });
			const report = { name: 'report', png: await page.screenshot({ type: 'png' }) };
			await page.close();

			return [terminal('player'), terminal('library'), terminal('mixes'), report];
		} finally {
			rmSync(work, { recursive: true, force: true });
		}
	}
};
