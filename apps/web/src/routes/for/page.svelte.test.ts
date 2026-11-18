import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { PAGE_META } from '#lib/testing/page-meta.ts';
import type { PageData } from './$types';
import Page from './+page.svelte';

const show = (stats: PageData['stats']) =>
	render(Page, { params: {}, form: null, data: { viewer: null, pendingRequests: 0, meta: PAGE_META, stats } });

describe('/for', () => {
	it('sends promoters and DJs each to their own page', async () => {
		await show(Promise.resolve({ djs: 12, cities: 10, unclaimed: 7 }));

		await expect
			.element(page.getByRole('heading', { level: 1 }))
			.toHaveTextContent('Which side of the booth are you on?');
		await expect.element(page.getByRole('link', { name: 'I put on shows.' })).toHaveAttribute('href', '/for/promoters');
		await expect.element(page.getByRole('link', { name: 'I play out.' })).toHaveAttribute('href', '/for/djs');
		await expect.element(page.getByText('12 DJs in 10 cities so far.')).toBeVisible();
	});

	it('points people who only want the music at shows, charts and Find DJs', async () => {
		await show(Promise.resolve(null));

		const music = page.getByRole('navigation', { name: 'Just here for the music' });
		await expect.element(music.getByRole('link', { name: 'Shows' })).toHaveAttribute('href', '/shows');
		await expect.element(music.getByRole('link', { name: 'Charts' })).toHaveAttribute('href', '/charts');
		await expect.element(music.getByRole('link', { name: 'Find DJs' })).toHaveAttribute('href', '/djs');
	});

	it('leaves the totals out while the directory is offline or empty', async () => {
		const { rerender } = await show(Promise.resolve(null));
		await expect.element(page.getByRole('link', { name: 'I play out.' })).toBeVisible();
		expect(page.getByText(/so far/).elements()).toEqual([]);

		await rerender({
			data: {
				viewer: null,
				pendingRequests: 0,
				meta: PAGE_META,
				stats: Promise.resolve({ djs: 0, cities: 0, unclaimed: 0 })
			}
		});
		await expect.element(page.getByRole('link', { name: 'I put on shows.' })).toBeVisible();
		expect(page.getByText(/so far/).elements()).toEqual([]);
	});
});
