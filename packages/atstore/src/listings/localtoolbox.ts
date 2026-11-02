// localtoolbox's captures: the built popup (dist/), served to a page with a stand-in for the few
// chrome.* calls it makes, opened over a made-up SoundCloud playlist (both in localtoolbox-page.ts),
// and the built content script marking that playlist's page (localtoolbox-marks.ts). Nothing is
// asked of SoundCloud or any other host: the artwork is drawn here too, and every request is
// answered here or refused.
import { readFileSync } from 'node:fs';
import { extname, join } from 'node:path';
import type { Browser, BrowserContext, Locator, Page } from 'playwright';
import { ROOT, run } from '../run.ts';
import type { Capture, Listing } from '../types.ts';
import { marksPage, marksStorage } from './localtoolbox-marks.ts';
import { ARTWORK_HOST, cover, installChrome, PAGE } from './localtoolbox-page.ts';

const DIST = join(ROOT, 'apps/localtoolbox/dist');

/** The popup's width (app.css), and the most height Chrome gives a popup. */
const POPUP = { width: 400, height: 600 };

/** The playlist page's windows: a laptop's and a phone's. */
const WINDOWS = [
	{ file: 'marks.png', viewport: { width: 1280, height: 800 } },
	{ file: 'marks-phone.png', viewport: { width: 390, height: 844 } }
];

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

/** Answers the context's requests: the popup's files, the playlist page and its artwork. Anything else is refused. */
async function serve(context: BrowserContext) {
	await context.route('**/*', (route) => {
		const url = route.request().url();

		if (url.startsWith(ORIGIN)) {
			const path = join(DIST, new URL(url).pathname);

			return route.fulfill({
				body: readFileSync(path),
				contentType: TYPES.get(extname(path)) ?? 'application/octet-stream'
			});
		}

		if (url.startsWith(ARTWORK_HOST)) return route.fulfill({ body: cover(url), contentType: 'image/svg+xml' });

		if (url === PAGE.tab.url) return route.fulfill({ body: marksPage(), contentType: 'text/html' });

		return route.abort('blockedbyclient');
	});
}

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
async function shot(page: Page, file: string): Promise<Capture> {
	await page.waitForFunction(() => !document.querySelector('[aria-live].fixed > *'));
	// The panels' entrance transitions, over.
	await page.waitForTimeout(800);
	const height = Math.min(POPUP.height, await page.locator('#app').evaluate((app) => app.scrollHeight));

	return { file, bytes: await page.screenshot({ type: 'png', clip: { x: 0, y: 0, width: POPUP.width, height } }) };
}

/** The popup's tools, each over the playlist. */
async function popup(browser: Browser): Promise<Capture[]> {
	const context = await browser.newContext({ viewport: POPUP, deviceScaleFactor: 2, colorScheme: 'dark' });

	try {
		await serve(context);
		await context.addInitScript(installChrome, PAGE);
		const page = await context.newPage();
		const shots: Capture[] = [];

		await open(page);
		await tool(page, /manager/i, (p) => p.getByText('Glass Hours'), { add: true });
		shots.push(await shot(page, 'manager.png'));

		await open(page);
		await tool(page, /playlist to text/i, (p) => p.getByRole('textbox', { name: 'the tracks as text' }));
		shots.push(await shot(page, 'playlist-to-text.png'));

		await open(page);
		await tool(page, /artists/i, (p) => p.getByText('Mira Okafor'), { add: true });
		shots.push(await shot(page, 'artists.png'));

		await open(page);
		shots.push(await shot(page, 'menu.png'));

		return shots;
	} finally {
		await context.close();
	}
}

/**
 * The playlist's page with the content script running on it, loaded as the manifest has Chrome
 * load it, and the pointer on a track's cover so its queue button shows.
 */
async function marks(browser: Browser, { file, viewport }: (typeof WINDOWS)[number]): Promise<Capture> {
	const context = await browser.newContext({ viewport, deviceScaleFactor: 2 });

	try {
		await serve(context);
		await context.addInitScript(installChrome, { ...PAGE, local: marksStorage(Date.now()) });
		const page = await context.newPage();
		await page.goto(PAGE.tab.url);

		const manifest: { content_scripts: { js: string[]; css: string[] }[] } = JSON.parse(
			readFileSync(join(DIST, 'manifest.json'), 'utf8')
		);

		for (const { js, css } of manifest.content_scripts) {
			for (const style of css) await page.addStyleTag({ path: join(DIST, style) });

			for (const script of js) await page.addScriptTag({ path: join(DIST, script) });
		}

		// The downloaded marks, the "nodl" one, and the player bar's.
		await page.locator('.tbx-mark').nth(5).waitFor();
		await page.locator('.trackList__item').nth(1).locator('.sc-artwork').hover();
		await page.locator('.tbx-queue').first().waitFor();
		await page.evaluate(() => document.fonts.ready);
		// The queue button's fade in, over.
		await page.waitForTimeout(400);

		return { file, bytes: await page.screenshot({ type: 'png' }) };
	} finally {
		await context.close();
	}
}

export const localtoolbox: Listing = {
	id: 'localtoolbox',
	name: 'localtoolbox',
	eyebrow: 'Local Network · Chrome extension',
	icon: 'apps/localtoolbox/assets/icon.svg',
	glow: '#ff5500',
	hero: { shots: ['manager', 'playlist-to-text'], frame: { kind: 'panel' } },
	screenshots: ['manager', 'playlist-to-text', 'artists', 'menu'],
	page: true,
	async capture(browser) {
		await run('pnpm', ['-C', 'apps/localtoolbox', 'build']);
		const shots = await popup(browser);

		for (const window of WINDOWS) shots.push(await marks(browser, window));

		return shots;
	}
};
