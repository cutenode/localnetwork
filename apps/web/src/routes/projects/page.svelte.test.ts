import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { PageData } from './$types';
import Page from './+page.svelte';
import { PAGE_META } from '#lib/testing/page-meta.ts';

const filters: PageData['filters'] = {
	q: '',
	place: '',
	lat: '',
	lng: '',
	radiusKm: 50,
	genres: [],
	kinds: [],
	formats: [],
	setTypes: [],
	feeTiers: [],
	country: '',
	sort: '',
	unclaimed: false
};

const result = (total: number) => ({ items: [], total, page: 1, pageSize: 24 });

const show = (
	data: Partial<{ filters: typeof filters; placeMissed: boolean; result: ReturnType<typeof result>; geocoder: boolean }>
) =>
	render(Page, {
		params: {},
		form: null,
		data: {
			viewer: null,
			pendingRequests: 0,
			meta: PAGE_META,
			filters,
			placeMissed: false,
			result: result(0),
			geocoder: true,
			...data
		}
	});

it("says when a typed place can't be found, instead of a distance", async () => {
	await show({ filters: { ...filters, place: 'Atlantis' }, placeMissed: true, result: result(2) });

	await expect.element(page.getByText(/Couldn't find “Atlantis”/)).toBeVisible();
	await expect.element(page.getByRole('heading', { name: '2 DJs' })).toBeVisible();
	await expect.element(page.getByText(/within 50 km/)).not.toBeInTheDocument();
});

it('says distance search is off when there is no geocoder to look places up', async () => {
	await show({ filters: { ...filters, place: 'Atlantis' }, placeMissed: true, geocoder: false });

	await expect
		.element(page.getByText("Distance search is off right now, so these aren't sorted by distance."))
		.toBeVisible();
});

it('shows the filters that are on, each with a way to drop it', async () => {
	await show({
		filters: { ...filters, place: 'Berlin', lat: '52.52', lng: '13.4', genres: ['techno'] },
		result: result(4)
	});

	await expect.element(page.getByRole('heading', { name: '4 DJs within 50 km of Berlin' })).toBeVisible();
	const on = page.getByRole('list', { name: 'Filters on' });
	await expect.element(on.getByRole('link', { name: 'Remove techno' })).toBeVisible();
	await expect.element(on.getByRole('link', { name: 'Remove Within 50 km of Berlin' })).toBeVisible();
});

it('suggests what to loosen, going by the filters in use', async () => {
	await show({ filters: { ...filters, genres: ['techno'] } });
	await expect.element(page.getByText('Try fewer genres.')).toBeVisible();
});

it('switches between searching and matching a show, with search current', async () => {
	await render(Page, {
		params: {},
		form: null,
		data: {
			viewer: null,
			pendingRequests: 0,
			meta: PAGE_META,
			filters,
			placeMissed: false,
			result: null,
			geocoder: false
		}
	});
	const modes = page.getByRole('navigation', { name: 'Find DJs' });

	await expect.element(modes.getByRole('link', { name: 'Search' })).toHaveAttribute('aria-current', 'page');
	await expect.element(modes.getByRole('link', { name: 'Match a show' })).toHaveAttribute('href', '/match');
	await expect.element(modes.getByRole('link', { name: 'Match a show' })).not.toHaveAttribute('aria-current');
	await expect.element(page.getByRole('heading', { level: 1, name: 'Find DJs' })).toBeInTheDocument();
});
