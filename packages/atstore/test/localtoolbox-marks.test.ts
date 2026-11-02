import { chromium, type Browser, type Page } from 'playwright';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { marksPage, marksStorage } from '../src/listings/localtoolbox-marks.ts';
import { ARTWORK_HOST, PAGE } from '../src/listings/localtoolbox-page.ts';

const { tracks } = PAGE.answers.EXTRACT_TRACKS;

describe('marksStorage', () => {
	const now = Date.parse('2026-11-01T12:00:00Z');
	const { downloaded, details } = marksStorage(now);

	it('marks some of the tracks downloaded, at the time given', () => {
		expect(Object.keys(downloaded).length).toBeGreaterThan(1);
		expect(Object.keys(downloaded).every((permalink) => tracks.some((t) => t.permalink === permalink))).toBe(true);
		expect(new Set(Object.values(downloaded))).toEqual(new Set(['2026-11-01T12:00:00.000Z']));
	});

	it('has every track looked up just now, and one of them with no download at all', () => {
		expect(Object.keys(details)).toEqual(tracks.map((t) => t.permalink));
		expect(Object.values(details).every((d) => d.at === now)).toBe(true);

		const none = Object.values(details).filter((d) => !d.downloadable && !d.purchaseUrl);
		expect(none.map((d) => d.permalink)).toEqual(['/velvet-arcade/night-bus-to-ostend']);
		expect(downloaded).not.toHaveProperty(none[0].permalink);
	});
});

describe('marksPage', () => {
	let browser: Browser;
	let page: Page;
	const asked: string[] = [];

	beforeAll(async () => {
		browser = await chromium.launch();
		page = await browser.newPage();
		await page.route('**/*', (route) => {
			asked.push(route.request().url());

			return route.abort();
		});
		await page.setContent(marksPage());
	});

	afterAll(async () => {
		await browser.close();
	});

	it('lists every track of the playlist, linked as a playlist page links them', async () => {
		const rows = page.locator('.trackList__item');
		await expect.poll(() => rows.count()).toBe(tracks.length);

		for (const [i, track] of tracks.entries()) {
			const title = rows.nth(i).getByRole('link', { name: track.title });
			expect(await title.getAttribute('href')).toBe(`${track.permalink}?in=halflight/sets/warm-up-october`);
		}
	});

	it('names the playlist and its curator, and plays the first track', async () => {
		await expect.poll(() => page.getByRole('heading', { name: 'warm-up, october' }).count()).toBe(1);
		await expect.poll(() => page.getByRole('heading', { name: /^Halflight/ }).count()).toBe(1);
		const playing = page.locator('.playControls').getByRole('link', { name: tracks[0].title });
		expect(await playing.getAttribute('href')).toBe(tracks[0].permalink);
	});

	it('asks for nothing but artwork', () => {
		expect(asked.length).toBeGreaterThan(tracks.length);
		expect(asked.filter((url) => !url.startsWith(ARTWORK_HOST))).toEqual([]);
	});
});
