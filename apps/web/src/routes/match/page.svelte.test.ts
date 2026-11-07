import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Page from './+page.svelte';
import { PAGE_META } from '#lib/testing/page-meta.ts';

const criteria = {
	title: '',
	date: '',
	place: '',
	lat: '',
	lng: '',
	location: undefined,
	radiusKm: 50,
	genres: [],
	budgetTier: undefined,
	capacity: undefined,
	setType: undefined,
	formats: []
};

it('switches between searching and matching a show, with matching current', async () => {
	await render(Page, {
		params: {},
		form: null,
		data: { viewer: null, pendingRequests: 0, meta: PAGE_META, criteria, searched: false, results: [], geocoder: false }
	});
	const modes = page.getByRole('navigation', { name: 'Find DJs' });

	await expect.element(modes.getByRole('link', { name: 'Match a show' })).toHaveAttribute('aria-current', 'page');
	await expect.element(modes.getByRole('link', { name: 'Search' })).toHaveAttribute('href', '/projects');
	await expect.element(modes.getByRole('link', { name: 'Search' })).not.toHaveAttribute('aria-current');
	await expect.element(page.getByRole('heading', { level: 1, name: 'Who fits your show?' })).toBeInTheDocument();
});
