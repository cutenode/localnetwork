import { page } from 'vitest/browser';
import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { PAGE_META } from '#lib/testing/page-meta.ts';
import ArtistLinks from '#lib/components/supports/ArtistLinks.svelte';
import Charts from './charts/+page.svelte';
import Shows from './shows/+page.svelte';
// The folded forms and the link style come from the site's stylesheet, so the tests draw with it.
import './layout.css';

const base = { viewer: null, pendingRequests: 0, meta: PAGE_META };

const berlin = { place: 'Berlin, Germany', lat: 52.52, lng: 13.405, radiusKm: 25 };

const shows = (place: typeof berlin | null) =>
	render(Shows, {
		params: {},
		form: null,
		data: {
			...base,
			following: false,
			place,
			// The radii the server offers (RADII in lib/server/shows-place.ts, which the browser can't import).
			radii: [10, 25, 50, 100, 250] as const,
			geocoder: true,
			shows: Promise.resolve({ items: [] })
		}
	});

const charts = () =>
	render(Charts, {
		params: {},
		form: null,
		data: {
			...base,
			filters: { month: null, place: 'Berlin, DE', query: { since: '2026-10-09' } },
			months: ['2026-11'],
			charts: Promise.resolve(null)
		}
	});

afterEach(() => page.viewport(414, 896));

describe('/shows', () => {
	it('calls the first tab what it lists: every show, or the ones near the city picked', async () => {
		await shows(null);
		const tabs = page.getByRole('navigation', { name: 'Which shows' });
		await expect.element(tabs.getByRole('link', { name: 'All shows' })).toHaveAttribute('aria-current', 'page');
		await expect.element(page.getByRole('heading', { name: 'Everywhere' })).toBeVisible();

		await shows(berlin);
		await expect.element(page.getByRole('link', { name: 'Near Berlin, Germany' })).toBeVisible();
	});

	it('folds the city picker on a phone, naming the city in use', async () => {
		await page.viewport(390, 844);
		await shows(berlin);

		await expect.element(page.getByText('Berlin, Germany', { exact: true }).first()).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Find shows' })).not.toBeInTheDocument();
	});
});

describe('/charts', () => {
	it('says what the charts are, with how they count a click away', async () => {
		await charts();

		await expect.element(page.getByText('The tracks the most DJs played.')).toBeVisible();
		await expect.element(page.getByText(/Each DJ counts once per track/)).not.toBeVisible();
		await page.getByText('How charts count').click();
		await expect.element(page.getByText(/Each DJ counts once per track/)).toBeVisible();
	});

	it('folds its filters on a phone, naming the ones in use', async () => {
		await page.viewport(390, 844);
		await charts();

		await expect.element(page.getByText('Last 30 days · Berlin, DE')).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Show charts' })).not.toBeInTheDocument();
	});
});

describe('entity links', () => {
	it('share one quiet underline', async () => {
		await render(ArtistLinks, { artists: [{ name: 'Ferrule' }] });

		const link = page.getByRole('link', { name: 'Ferrule' }).element();

		// The line token as the browser resolves it, to compare the underline with.
		const swatch = document.createElement('span');
		swatch.style.color = 'var(--color-line-strong)';
		document.body.append(swatch);

		expect(getComputedStyle(link).textDecorationLine).toBe('underline');
		expect(getComputedStyle(link).textDecorationColor).toBe(getComputedStyle(swatch).color);
		swatch.remove();
	});
});
