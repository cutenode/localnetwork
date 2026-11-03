// The VHS tape locallibrary's terminal captures are recorded from, and the players it leaves
// behind when VHS doesn't get to the end of it.
import { execFileSync, spawnSync } from 'node:child_process';
import { join } from 'node:path';

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

/** The views the tape screenshots, in its order. */
export const VIEWS = ['library', 'drives', 'snapshots', 'mixes', 'check', 'player'];

/** A terminal a tape is recorded in, in pixels, and the views it screenshots. */
interface Terminal {
	suffix: string;
	width: number;
	height: number;
	fontSize: number;
	padding: number;
	views: string[];
}

/**
 * The terminals the tapes are recorded in, at the website's ratios, each font as large as its
 * views allow: the website shows the wide ones about 750 CSS pixels wide, and phones' 390.
 */
export const TERMINALS: Terminal[] = [
	{ suffix: '', width: 2560, height: 1600, fontSize: 38, padding: 40, views: VIEWS },
	{
		suffix: '-phone',
		width: 780,
		height: 1400,
		fontSize: 21,
		padding: 16,
		views: ['drives', 'snapshots', 'mixes', 'player']
	},
	// The library's track list needs 65 columns, or it cuts its titles and artists to a letter.
	{ suffix: '-phone', width: 780, height: 1400, fontSize: 18, padding: 16, views: ['library'] },
	// `check` prints lines that wrap anywhere, so it can be as large as its output fits.
	{ suffix: '-phone', width: 780, height: 1400, fontSize: 26, padding: 16, views: ['check'] }
];

/**
 * A tape for `terminal`: on DJ (whose recording folder holds the set), the library, the Drives
 * view with DJ picked, the snapshots and the set in the Mixes view; DJ checked from the shell; then,
 * on DEMO, a track playing full screen. DEMO is rekordbox's demo export, whose two tracks come
 * with their waveforms, beat grid and phrases. It screenshots the terminal's views alone. A
 * screenshot is of the frame after it, so each is followed by a pause.
 */
export function tape(bin: string, env: Record<string, string>, volumes: string, terminal: Terminal): string {
	const open = (drive: string) =>
		`Type ${JSON.stringify(`clear; locallibrary tui ${join(volumes, drive)}`)}\nEnter\nSleep 5s`;

	// Quit, or the player outlives the recording and keeps the stick from ejecting.
	const quit = 'Ctrl+C\nSleep 2s';

	const shot = (name: string) =>
		terminal.views.includes(name) ? `Show\nSleep 1s\nScreenshot "${name}${terminal.suffix}.png"\nSleep 1s\nHide` : [];

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
		quit,
		'Type "clear"\nEnter\nSleep 1s',
		'Type "locallibrary check DJ --players CDJ-3000,CDJ-2000NXS2"\nEnter\nSleep 5s',
		shot('check'),
		open('DEMO'),
		'Type "1"\nSleep 1s\nEnter\nSleep 6s\nType "f"\nSleep 3s',
		shot('player'),
		quit
	]
		.flat()
		.join('\n');
}

/**
 * Kills the players a tape opened on the drives in `work` that are still running, as VHS leaves
 * them when it fails or is stopped partway, and returns their pids. A player whose terminal has
 * closed can miss SIGTERM, so it's SIGKILL, by kill(1), which shrugs off one that has just ended.
 */
export function killPlayers(work: string): number[] {
	const players = execFileSync('ps', ['-Ao', 'pid=,args='], { encoding: 'utf8' })
		.split('\n')
		.filter((line) => line.includes(` tui ${work}/`))
		.map((line) => Number.parseInt(line, 10));

	if (players.length > 0) spawnSync('kill', ['-KILL', ...players.map(String)]);

	return players;
}
