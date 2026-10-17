import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { imageSize } from 'image-size';
import { chromium, type Browser } from 'playwright';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { HERO, heroHtml, ICON_SIZE, renderHero, renderIcon, snap } from '../src/render.ts';
import { ROOT } from '../src/run.ts';
import type { Listing } from '../src/types.ts';

const svg = readFileSync(join(ROOT, 'apps/locallibrary/assets/icon.svg'), 'utf8');

const listing = (frame: Listing['hero']['frame']): Listing => ({
	id: 'test',
	name: 'Tom & <Jerry>',
	eyebrow: 'Local Network · "test"',
	icon: 'apps/locallibrary/assets/icon.svg',
	glow: '#ff5ca0',
	hero: { shots: [0], frame },
	capture: async () => []
});

const shot = { name: 'one', png: Buffer.from('not really a png') };

describe('heroHtml', () => {
	it('escapes the words it is given', () => {
		const html = heroHtml(listing({ kind: 'browser', url: 'example.com/?a=1&b=<2>' }), svg, [shot]);
		expect(html).toContain('<h1>Tom &amp; &lt;Jerry&gt;</h1>');
		expect(html).toContain('Local Network · &quot;test&quot;');
		expect(html).toContain('<span class="address">example.com/?a=1&amp;b=&lt;2&gt;</span>');
	});

	it('puts each screenshot in the frame asked for', () => {
		const terminal = heroHtml(listing({ kind: 'terminal', title: 'locallibrary' }), svg, [shot, shot]);
		expect(terminal.match(/<figure><div class="bar">/g)).toHaveLength(2);
		expect(terminal).toContain('<span>locallibrary</span>');
		expect(terminal).toContain('class="stage terminal"');

		const panel = heroHtml(listing({ kind: 'panel' }), svg, [shot]);
		expect(panel).not.toContain('class="bar"');
		expect(panel).toContain('<figure><img src="data:image/png;base64,');
	});
});

describe('drawing', () => {
	let browser: Browser;

	beforeAll(async () => {
		browser = await chromium.launch();
	});

	afterAll(async () => {
		await browser.close();
	});

	it('draws the icon edge to edge, at its size', async () => {
		const png = await renderIcon(browser, svg);
		expect(imageSize(png)).toMatchObject({ type: 'png', width: ICON_SIZE, height: ICON_SIZE });
	});

	it('draws the hero at 16:9', async () => {
		const page = await browser.newPage();
		await page.setContent('<body style="margin:0;background:#123456;width:200px;height:100px"></body>');
		const screenshot = { name: 'blue', png: await page.screenshot() };
		await page.close();

		const png = await renderHero(browser, listing({ kind: 'browser', url: 'localnetwork.computer' }), svg, [
			screenshot
		]);

		expect(imageSize(png)).toMatchObject({ width: HERO.width, height: HERO.height });
	});

	it('draws at the scale asked for', async () => {
		expect(imageSize(await snap(browser, '<p>hi</p>', 100, 50, 2))).toMatchObject({ width: 200, height: 100 });
	});
});
