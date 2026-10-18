// The icon and the hero, drawn by Chromium from HTML: the same engine and fonts as the website, so
// the pictures look like the rest of Local Network.
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import type { Browser } from 'playwright';
import type { Frame, Listing, Shot } from './types.ts';

/** The icon's side. The AT Store crops icons to a square and rounds the corners itself. */
export const ICON_SIZE = 1024;

/** The hero's size: 16:9, which the AT Store shows it at. */
export const HERO = { width: 1920, height: 1080 };

const require = createRequire(import.meta.url);

const dataUrl = (type: string, bytes: Buffer | string) =>
	`data:${type};base64,${Buffer.from(bytes).toString('base64')}`;

const font = (file: string) => dataUrl('font/woff2', readFileSync(require.resolve(`@fontsource-variable/${file}`)));

const escape = (text: string) =>
	text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');

/** A PNG of `html`, laid out in a window `width` by `height` CSS pixels and drawn at `scale`. */
export async function snap(browser: Browser, html: string, width: number, height: number, scale = 1): Promise<Buffer> {
	const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: scale });

	try {
		await page.setContent(html, { waitUntil: 'load' });
		await page.evaluate(() => document.fonts.ready);

		return await page.screenshot({ type: 'png' });
	} finally {
		await page.close();
	}
}

/** The icon: the SVG drawn edge to edge at ICON_SIZE. */
export function renderIcon(browser: Browser, svg: string): Promise<Buffer> {
	const html = `<style>html,body{margin:0}img{display:block}</style><img src="${dataUrl('image/svg+xml', svg)}" width="${ICON_SIZE}" height="${ICON_SIZE}" alt="">`;

	return snap(browser, html, ICON_SIZE, ICON_SIZE);
}

/** The window chrome over a screenshot: dots and the address for a browser, dots and a title for a terminal. */
function bar(frame: Frame): string {
	if (frame.kind === 'panel') return '';
	const dots = '<i></i><i></i><i></i>';

	const label =
		frame.kind === 'browser'
			? `<span class="address">${escape(frame.url)}</span>`
			: `<span>${escape(frame.title)}</span>`;

	return `<div class="bar">${dots}${label}</div>`;
}

/** The hero's HTML: the icon, eyebrow and name on the left, the screenshots in their frame running off the right. */
export function heroHtml(listing: Listing, svg: string, shots: Shot[]): string {
	const { frame } = listing.hero;

	const windows = shots
		.map((shot) => `<figure>${bar(frame)}<img src="${dataUrl('image/png', shot.png)}" alt=""></figure>`)
		.join('');

	return `<!doctype html>
<meta charset="utf-8">
<style>
@font-face { font-family: 'Instrument Sans'; font-weight: 400 700; src: url(${font('instrument-sans/files/instrument-sans-latin-wght-normal.woff2')}) format('woff2'); }
@font-face { font-family: 'JetBrains Mono'; font-weight: 100 800; src: url(${font('jetbrains-mono/files/jetbrains-mono-latin-wght-normal.woff2')}) format('woff2'); }
html, body { margin: 0; width: ${HERO.width}px; height: ${HERO.height}px; overflow: hidden; }
body {
	position: relative;
	background: radial-gradient(ellipse 70% 80% at 78% 92%, color-mix(in srgb, ${listing.glow} 34%, transparent), transparent 70%), #09090b;
	color: #f4f4f5;
	font-family: 'Instrument Sans', system-ui, sans-serif;
}
.grid {
	position: absolute; inset: 0;
	background-image: linear-gradient(#16161a 1px, transparent 1px), linear-gradient(90deg, #16161a 1px, transparent 1px);
	background-size: 64px 64px;
	mask-image: radial-gradient(ellipse 80% 90% at 30% 40%, black, transparent 85%);
}
.intro { position: absolute; left: 128px; top: 160px; width: 600px; }
.icon { display: block; width: 144px; height: 144px; border-radius: 34px; box-shadow: 0 0 0 1px #2c2c33, 0 24px 64px -16px #000; }
.eyebrow { margin: 64px 0 0; font: 500 22px/1.4 'JetBrains Mono', ui-monospace, monospace; letter-spacing: 0.08em; text-transform: uppercase; color: #a1a1aa; }
h1 { margin: 20px 0 0; font-size: 112px; font-weight: 600; line-height: 0.98; letter-spacing: -0.045em; }
.stage { position: absolute; left: 800px; top: 136px; display: flex; gap: 48px; align-items: flex-start; }
figure {
	margin: 0; overflow: hidden; flex: none;
	background: #0e0e11; border: 1px solid #2c2c33; border-radius: 16px;
	box-shadow: 0 -40px 120px -40px color-mix(in srgb, ${listing.glow} 40%, transparent), 0 40px 120px -20px #000;
}
.browser figure, .terminal figure { width: 1360px; }
.panel figure { width: 440px; }
.panel figure:nth-child(2) { margin-top: 96px; }
figure img { display: block; width: 100%; }
.bar { display: flex; align-items: center; gap: 10px; padding: 16px 20px; border-bottom: 1px solid #1f1f24; }
.bar i { width: 13px; height: 13px; border-radius: 50%; background: #2c2c33; }
.bar span { margin: 0 auto; font: 18px 'JetBrains Mono', ui-monospace, monospace; color: #8b8b94; }
.bar .address { padding: 5px 18px; border-radius: 6px; background: #16161a; }
</style>
<div class="grid"></div>
<div class="intro">
	<img class="icon" src="${dataUrl('image/svg+xml', svg)}" alt="">
	<p class="eyebrow">${escape(listing.eyebrow)}</p>
	<h1>${escape(listing.name)}</h1>
</div>
<div class="stage ${frame.kind}">${windows}</div>`;
}

/** The hero, drawn at HERO's size. */
export function renderHero(browser: Browser, listing: Listing, svg: string, shots: Shot[]): Promise<Buffer> {
	return snap(browser, heroHtml(listing, svg, shots), HERO.width, HERO.height);
}
