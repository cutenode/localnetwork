import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';

/**
 * The browser the TUI stories are drawn and screenshotted in: Playwright's own Linux image, the same
 * Chromium on every machine and in CI, so a story's pixels don't depend on the platform its tests ran
 * on. Started for a run (this module is the TUI projects' global setup) and stopped after; with
 * `TUI_BROWSER` set to a browser server's address, that one is used and nothing is started.
 */

// A name and port of this checkout's own, so a run in another worktree neither takes this one's port nor removes
// its browser.
const checkout = createHash('sha256')
	.update(import.meta.dirname)
	.digest('hex');

const NAME = `locallibrary-tui-browser-${checkout.slice(0, 8)}`;

/** Where the browser server listens on this machine. */
export const TUI_BROWSER_PORT = 53_000 + (Number.parseInt(checkout.slice(0, 8), 16) % 1000);

export const TUI_BROWSER = process.env.TUI_BROWSER ?? `ws://127.0.0.1:${TUI_BROWSER_PORT}/`;

/** Whether there's a pinned browser to draw in: one given, or Docker to run one. Without it the TUI stories run in
 *  this machine's Chromium and their screenshots aren't compared, since this machine's pixels aren't CI's. */
export const PINNED =
	Boolean(process.env.TUI_BROWSER) ||
	(() => {
		try {
			execFileSync('docker', ['version'], { stdio: 'ignore', timeout: 10_000 });

			return true;
		} catch {
			return false;
		}
	})();

/** The Chromium flags that keep a screenshot the same from run to run: software WebGL (xterm draws
 *  with it), one device pixel per CSS pixel, no font hinting, sRGB and no audio clock. */
export const TUI_BROWSER_ARGS = [
	'--use-angle=swiftshader',
	'--enable-unsafe-swiftshader',
	'--force-device-scale-factor=1',
	'--font-render-hinting=none',
	'--force-color-profile=srgb',
	'--disable-audio-output'
];

/** The label that says which process started the container: it alone removes it. */
const OWNER = 'locallibrary.tui-browser.owner';

/** npm's download cache, kept between containers: each one installs Playwright's server with npx, which without the
 *  cache downloads it first, and in an emulated container on a busy machine that alone took minutes. Only the
 *  content-addressed cache is shared, which two checkouts' containers can fill at once. */
const NPM_CACHE = 'locallibrary-tui-browser-npm';

/** The image of the Playwright the tests use: its server and its client must be the same version. */
const image = () => {
	// SAFETY: it's playwright's own package.json, and every package.json has a version.
	const { version } = createRequire(import.meta.url)('playwright/package.json') as { version: string };

	return { version, image: `mcr.microsoft.com/playwright:v${version}-noble` };
};

const docker = (...args: string[]) => execFileSync('docker', args, { encoding: 'utf8', stdio: 'pipe' });

/** Whether the server in the container says it's listening (Docker takes connections on the port
 *  before anything in the container does). */
const listening = () => {
	try {
		return docker('logs', NAME).includes('Listening on ws://');
	} catch {
		return false;
	}
};

/** The process that started this checkout's container, if there is one. */
const owner = () => {
	try {
		return Number(docker('inspect', '--format', `{{index .Config.Labels "${OWNER}"}}`, NAME).trim());
	} catch {
		return undefined;
	}
};

const alive = (pid: number) => {
	// No label (0 or NaN): not ours to keep. Signalling 0 would reach this whole process group.
	if (!(pid > 0)) return false;

	try {
		process.kill(pid, 0);

		return true;
	} catch (error) {
		// SAFETY: process.kill throws a system error with its code. EPERM: it's there, run by someone else.
		return (error as NodeJS.ErrnoException).code === 'EPERM';
	}
};

export default async function setup() {
	if (process.env.TUI_BROWSER) return;

	if (!PINNED) {
		console.warn(
			"TUI stories: Docker isn't running, so their play tests run here and their screenshots aren't compared"
		);

		return;
	}

	const { version, image: from } = image();
	const running = owner();
	// The other TUI project's setup in this run, or another run in this checkout, started it: use it, and leave it
	// to them to remove. One whose run was killed before it could clean up is replaced.
	const shared = running !== undefined && alive(running);

	if (running !== undefined && !shared) docker('rm', '--force', NAME);

	if (!shared)
		execFileSync(
			'docker',
			[
				'run',
				'--detach',
				'--rm',
				'--name',
				NAME,
				'--label',
				`${OWNER}=${process.pid}`,
				// The build CI's runners (amd64) use, also on an arm64 Mac (emulated), so the saved screenshots are drawn
				// by the Chromium that checks them.
				'--platform',
				'linux/amd64',
				'--init',
				'--publish',
				`127.0.0.1:${TUI_BROWSER_PORT}:3000`,
				'--volume',
				`${NPM_CACHE}:/root/.npm/_cacache`,
				from,
				'npx',
				'--yes',
				`playwright@${version}`,
				'run-server',
				'--port',
				'3000',
				'--host',
				'0.0.0.0'
			],
			{ stdio: ['ignore', 'ignore', 'inherit'] }
		);
	// The image is pulled once; the server then takes a few seconds to start.
	const started = Date.now();

	while (!listening()) {
		if (Date.now() - started > 120_000) {
			// A container left behind would be taken for this checkout's next run's, so the one started here goes.
			if (!shared) docker('rm', '--force', NAME);

			throw new Error(`the TUI browser (${from}) didn't start`);
		}

		await new Promise((r) => setTimeout(r, 250));
	}

	if (shared) return;

	return () => {
		docker('rm', '--force', NAME);
	};
}
