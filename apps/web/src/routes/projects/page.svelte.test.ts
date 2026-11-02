import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Page from './+page.svelte';
import { PAGE_META } from '#lib/testing/page-meta.ts';

const filters = {
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

it('switches between searching and matching a show, with search current', async () => {
	await render(Page, {
		params: {},
		form: null,
		data: { viewer: null, pendingRequests: 0, meta: PAGE_META, filters, result: null, geocoder: false }
	});
	const modes = page.getByRole('navigation', { name: 'Find DJs' });

	await expect.element(modes.getByRole('link', { name: 'Search' })).toHaveAttribute('aria-current', 'page');
	await expect.element(modes.getByRole('link', { name: 'Match a show' })).toHaveAttribute('href', '/match');
	await expect.element(modes.getByRole('link', { name: 'Match a show' })).not.toHaveAttribute('aria-current');
	await expect.element(page.getByRole('heading', { level: 1, name: 'Find DJs' })).toBeInTheDocument();
});
