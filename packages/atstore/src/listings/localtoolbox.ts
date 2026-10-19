// localtoolbox's screenshots: the built popup (dist/), served to a page with a stand-in for the
// few chrome.* calls it makes, opened over a made-up SoundCloud playlist (both in localtoolbox-page.ts).
// Nothing is asked of SoundCloud: the artwork is drawn here too.
import { readFileSync } from 'node:fs';
import { extname, join } from 'node:path';
import type { Locator, Page } from 'playwright';
import { ROOT, run } from '../run.ts';
import type { Listing, Shot } from '../types.ts';
import { ARTWORK_HOST, cover, installChrome, PAGE } from './localtoolbox-page.ts';

const DIST = join(ROOT, 'apps/localtoolbox/dist');

/** The popup's width (app.css), and the most height Chrome gives a popup. */
const POPUP = { width: 400, height: 600 };

/** Where the popup is served from; the stand-in is installed there before its scripts run. */
const ORIGIN = 'https://localtoolbox.test/';

/** The type each kind of file in dist/ is served as. */
const TYPES = new Map([
	['.html', 'text/html'],
	['.js', 'text/javascript'],
	['.mjs', 'text/javascript'],
	['.css', 'text/css'],
	['.png', 'image/png'],
	['.json', 'application/json']
]);

/** Opens the popup afresh, with an empty browsing session, and waits for its menu. */
async function open(page: Page) {
	await page.goto(`${ORIGIN}popup.html`);
	await page.getByRole('button', { name: /playlist to text/i }).waitFor();
}

/**
 * Opens the tool whose menu entry is called `name`, adds the page's tracks to its list if it
 * keeps one, and waits for what `shows` finds to be on screen.
 */
async function tool(page: Page, name: RegExp, shows: (page: Page) => Locator, { add = false } = {}) {
	await page.getByRole('button', { name }).click();

	if (add) await page.getByRole('button', { name: /^add \d+/ }).click();
	await shows(page).first().waitFor();
	await page.waitForLoadState('networkidle');
}

/** The popup as Chrome draws it: as tall as what's in it, up to POPUP's height, after its toasts have gone. */
async function shot(page: Page, name: string): Promise<Shot> {
	await page.waitForFunction(() => !document.querySelector('[aria-live].fixed > *'));
	// The panels' entrance transitions, over.
	await page.waitForTimeout(800);
	const height = Math.min(POPUP.height, await page.locator('#app').evaluate((app) => app.scrollHeight));

	return { name, png: await page.screenshot({ type: 'png', clip: { x: 0, y: 0, width: POPUP.width, height } }) };
}

export const localtoolbox: Listing = {
	id: 'localtoolbox',
	name: 'localtoolbox',
	eyebrow: 'Local Network · Chrome extension',
	icon: 'apps/localtoolbox/assets/icon.svg',
	glow: '#ff5500',
	hero: { shots: [0, 1], frame: { kind: 'panel' } },
	async capture(browser) {
		await run('pnpm', ['-C', 'apps/localtoolbox', 'build']);
		const context = await browser.newContext({ viewport: POPUP, deviceScaleFactor: 2, colorScheme: 'dark' });

		try {
			await context.route(`${ORIGIN}**`, (route) => {
				const path = join(DIST, new URL(route.request().url()).pathname);

				return route.fulfill({
					body: readFileSync(path),
					contentType: TYPES.get(extname(path)) ?? 'application/octet-stream'
				});
			});
			await context.route(`${ARTWORK_HOST}**`, (route) =>
				route.fulfill({ body: cover(route.request().url()), contentType: 'image/svg+xml' })
			);
			await context.addInitScript(installChrome, PAGE);
			const page = await context.newPage();
			const shots: Shot[] = [];

			await open(page);
			await tool(page, /manager/i, (p) => p.getByText('Glass Hours'), { add: true });
			shots.push(await shot(page, 'manager'));

			await open(page);
			await tool(page, /playlist to text/i, (p) => p.getByRole('textbox', { name: 'the tracks as text' }));
			shots.push(await shot(page, 'playlist-to-text'));

			await open(page);
			await tool(page, /artists/i, (p) => p.getByText('Mira Okafor'), { add: true });
			shots.push(await shot(page, 'artists'));

			await open(page);
			shots.push(await shot(page, 'menu'));

			return shots;
		} finally {
			await context.close();
		}
	}
};
