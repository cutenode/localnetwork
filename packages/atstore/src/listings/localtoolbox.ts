// localtoolbox's captures: the built popup (dist/), served to a page with a stand-in for the few
// chrome.* calls it makes, opened over a made-up SoundCloud playlist (both in localtoolbox-page.ts),
// and the built content script marking that playlist's page (localtoolbox-marks.ts). Nothing is
// asked of SoundCloud or any other host: the artwork is drawn here too, and every request is
// answered here or refused.
import { readFileSync } from 'node:fs';
import { extname, join } from 'node:path';
import type { Browser, BrowserContext, BrowserContextOptions, Locator, Page } from 'playwright';
import { ROOT, run } from '../run.ts';
import type { Capture, Listing } from '../types.ts';
import { MARKS, marksPage, marksStorage } from './localtoolbox-marks.ts';
import { ARTWORK_HOST, cover, installChrome, PAGE } from './localtoolbox-page.ts';

const DIST = join(ROOT, 'apps/localtoolbox/dist');

/** The popup's width (app.css), and the most height Chrome gives a popup. */
const POPUP = { width: 400, height: 600 };

/** A window to draw captures in, and how much of it they show, from the left of the playlist's rows, if not all of it. */
type Window = BrowserContextOptions & { clip?: { width: number; height: number } };

/**
 * The windows the website's captures are drawn in, at its ratios: 8:5 drawn 2560 by 1600, and
 * 39:70 drawn 780 by 1400 for phones. Each is as small as what it shows allows, so the popup's
 * text and the marks' ticks read at the 750 and 390 CSS pixels the website shows them at.
 */
const WINDOWS = {
	/** A small laptop's, as tall as a popup can be with the toolbar above it. */
	popup: { viewport: { width: 1000, height: 625 }, deviceScaleFactor: 2.56 },
	/** Just wider than the popup. */
	popupPhone: { viewport: { width: 429, height: 770 }, deviceScaleFactor: 780 / 429 },
	/** A laptop's, cut to the left of the playlist's rows. */
	marks: { viewport: { width: 1280, height: 800 }, deviceScaleFactor: 4, clip: { width: 640, height: 400 } },
	/** A narrow phone's. */
	marksPhone: { viewport: { width: 312, height: 560 }, deviceScaleFactor: 2.5 }
} satisfies Record<string, Window>;

/** When the pages think it is: fixed, so the marks' storage is fresh to the content script, and the same every run. */
const NOW = Date.parse('2026-11-01T12:00:00Z');

/** How Chrome draws a popup over the page: under the toolbar, its extension's button above its right edge. */
const BUBBLE = 'border-radius: 8px; box-shadow: 0 0 0 1px rgb(0 0 0 / 0.08), 0 6px 24px rgb(0 0 0 / 0.3);';

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

/** A popup's screenshot, by the tool it shows. */
interface Popup {
	name: string;
	png: Buffer;
	/** Its height, in CSS pixels. */
	height: number;
}

/**
 * A context with every request answered or refused, the clock fixed, and the stand-in for chrome.*
 * installed, with `local` in its storage.
 */
async function newContext(
	browser: Browser,
	options: BrowserContextOptions,
	local: ReturnType<typeof marksStorage> | Record<string, never> = {}
): Promise<BrowserContext> {
	const context = await browser.newContext(options);
	await context.clock.setFixedTime(NOW);
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
	await context.addInitScript(installChrome, { ...PAGE, local });

	return context;
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
async function shot(page: Page, name: string): Promise<Popup> {
	await page.waitForFunction(() => !document.querySelector('[aria-live].fixed > *'));
	// The panels' entrance transitions, over.
	await page.waitForTimeout(800);
	const height = Math.min(POPUP.height, await page.locator('#app').evaluate((app) => app.scrollHeight));
	const png = await page.screenshot({ type: 'png', clip: { x: 0, y: 0, width: POPUP.width, height } });

	return { name, png, height };
}

/** The popup's tools, each over the playlist, drawn as sharp as the website's wide captures. */
async function popups(browser: Browser): Promise<Popup[]> {
	const ctx = await newContext(browser, {
		viewport: POPUP,
		deviceScaleFactor: WINDOWS.popup.deviceScaleFactor,
		colorScheme: 'dark'
	});

	try {
		const page = await ctx.newPage();
		const shots: Popup[] = [];

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
		await ctx.close();
	}
}

/**
 * The playlist's page in `window`, with the content script marking it, loaded as the manifest has
 * Chrome load it, once every mark is on.
 */
async function marked(browser: Browser, options: BrowserContextOptions): Promise<{ ctx: BrowserContext; page: Page }> {
	const ctx = await newContext(browser, options, marksStorage(NOW));
	const page = await ctx.newPage();
	await page.goto(PAGE.tab.url);

	const manifest: { content_scripts: { js: string[]; css: string[] }[] } = JSON.parse(
		readFileSync(join(DIST, 'manifest.json'), 'utf8')
	);

	for (const { js, css } of manifest.content_scripts) {
		for (const style of css) await page.addStyleTag({ path: join(DIST, style) });

		for (const script of js) await page.addScriptTag({ path: join(DIST, script) });
	}

	await page
		.locator('.tbx-mark')
		.nth(MARKS - 1)
		.waitFor();
	const drawn = await page.locator('.tbx-mark').count();

	if (drawn !== MARKS) throw new Error(`the content script drew ${drawn} marks, not ${MARKS}`);
	await page.evaluate(() => document.fonts.ready);

	return { ctx, page };
}

/** Shows a popup's PNG, `src`, in `img`: `width` by `height` CSS pixels, styled `style`. Runs in the page. */
function draw(
	img: HTMLImageElement,
	{ src, width, height, style }: { src: string; width: number; height: number; style: string }
) {
	Object.assign(img, { src, width, height });
	img.setAttribute('style', style);

	return img.decode();
}

/**
 * Each popup over the marked playlist page, top right under the toolbar as Chrome draws it, and
 * alone for phones, on nothing, so it's centred on whatever the website shows it on.
 */
async function framed(browser: Browser, shots: Popup[]): Promise<Capture[]> {
	const captures: Capture[] = [];
	const { ctx, page } = await marked(browser, WINDOWS.popup);
	const alone = await browser.newPage(WINDOWS.popupPhone);
	const over = `position: fixed; z-index: 2147483647; top: 2px; right: 12px; ${BUBBLE}`;

	try {
		await page.evaluate(() => document.body.append(Object.assign(document.createElement('img'), { id: 'popup' })));
		await alone.setContent(
			'<body style="display: grid; place-items: center; height: 100vh; margin: 0"><img id="popup">'
		);

		for (const { name, png, height } of shots) {
			const src = `data:image/png;base64,${png.toString('base64')}`;
			await page.locator('#popup').evaluate(draw, { src, width: POPUP.width, height, style: over });
			captures.push({ file: `${name}.png`, bytes: await page.screenshot({ type: 'png' }) });

			await alone.locator('#popup').evaluate(draw, { src, width: POPUP.width, height, style: BUBBLE });
			const phone = await alone.screenshot({ type: 'png', omitBackground: true });
			captures.push({ file: `${name}-phone.png`, bytes: phone });
		}

		return captures;
	} finally {
		await alone.close();
		await ctx.close();
	}
}

/**
 * The marks, zoomed in on the playlist's rows below the top bar, with the pointer on a track's
 * cover so its queue button shows.
 */
async function marks(browser: Browser, file: string, { clip, ...options }: Window): Promise<Capture> {
	const { ctx, page } = await marked(browser, options);

	try {
		const top = await page.locator('.top').evaluate((bar) => bar.getBoundingClientRect().bottom);
		await page
			.locator('.listenEngagement')
			.evaluate((rows, under) => window.scrollBy(0, rows.getBoundingClientRect().top - under), top);
		await page.locator('.trackList__item').nth(1).locator('.sc-artwork').hover();
		await page.locator('.tbx-queue').first().waitFor();
		// The queue button's fade in, over.
		await page.waitForTimeout(400);
		// The cut starts at the rows' bar, wherever a page too short to scroll it to the top left it.
		const { x, y } = await page.locator('.listenEngagement').evaluate((bar) => bar.getBoundingClientRect());

		return {
			file,
			bytes: await page.screenshot({ type: 'png', clip: clip && { x: x - 24, y, ...clip } })
		};
	} finally {
		await ctx.close();
	}
}

export const localtoolbox: Listing = {
	id: 'localtoolbox',
	name: 'localtoolbox',
	eyebrow: 'Local Network · Chrome extension',
	icon: 'apps/localtoolbox/assets/icon.svg',
	glow: '#ff5500',
	hero: { shots: ['manager-phone', 'playlist-to-text-phone'], frame: { kind: 'panel' } },
	screenshots: ['manager', 'playlist-to-text', 'artists', 'menu'],
	page: true,
	async capture(browser, signal) {
		await run('pnpm', ['-C', 'apps/localtoolbox', 'build'], { signal });

		return [
			...(await framed(browser, await popups(browser))),
			await marks(browser, 'marks.png', WINDOWS.marks),
			await marks(browser, 'marks-phone.png', WINDOWS.marksPhone)
		];
	}
};
