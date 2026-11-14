import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { INQUIRIES, promoterPage } from './fixtures.ts';
import Page from './+page.svelte';

const region = (name: string) => page.getByRole('region', { name });

describe('/dashboard/promoter', () => {
	it('searches Find DJs, and links the matcher and the promoters page', async () => {
		await render(Page, { params: {}, form: null, data: promoterPage() });

		expect(page.getByRole('search', { name: 'Find DJs' }).element().getAttribute('action')).toBe('/projects');
		await expect.element(page.getByRole('link', { name: 'Match a show' })).toHaveAttribute('href', '/match');
		await expect
			.element(page.getByRole('link', { name: "What's new for promoters" }))
			.toHaveAttribute('href', '/for-promoters');
	});

	it('lists the inquiries sent, each to its thread, and how many more there are', async () => {
		await render(Page, { params: {}, form: null, data: promoterPage({ inquiryCount: 5 }) });

		const inquiries = region('Your inquiries');
		await expect
			.element(inquiries.getByRole('link', { name: 'Warehouse closing set' }))
			.toHaveAttribute('href', '/dashboard/bookings/3k1');
		await expect.element(inquiries.getByText('Negotiating')).toBeVisible();
		await expect.element(inquiries.getByText(/2 more inquiries in/)).toBeVisible();
	});

	it('lists the planned shows with their lineups', async () => {
		await render(Page, { params: {}, form: null, data: promoterPage() });

		const planned = region('Planned shows');
		await expect
			.element(planned.getByRole('link', { name: 'Warehouse' }))
			.toHaveAttribute('href', '/dashboard/planner/shows/3s1');
		await expect.element(planned.getByText(/The Dock · 3 DJs/)).toBeVisible();
		await expect.element(planned.getByText('No date yet · 0 DJs')).toBeVisible();
	});

	it('says what to do when nothing is asked or planned yet', async () => {
		await render(Page, {
			params: {},
			form: null,
			data: promoterPage({
				inquiries: [],
				inquiryCount: 0,
				planner: Promise.resolve({ available: true, shows: [], total: 0 })
			})
		});

		await expect.element(page.getByText('No inquiries yet')).toBeVisible();
		await expect.element(page.getByRole('link', { name: 'All bookings' })).not.toBeInTheDocument();
		await expect.element(page.getByText('No shows planned')).toBeVisible();
		await expect.element(page.getByRole('link', { name: 'Open the matcher' })).toHaveAttribute('href', '/match');
	});

	it('holds the planned shows while they load, then says when the planner is unavailable', async () => {
		const { rerender } = await render(Page, {
			params: {},
			form: null,
			data: promoterPage({ planner: new Promise<never>(() => {}) })
		});

		await expect.element(region('Planned shows').getByRole('list')).toHaveAttribute('aria-busy', 'true');

		await rerender({
			data: promoterPage({
				inquiries: INQUIRIES.slice(0, 1),
				planner: Promise.resolve({ available: false, problem: 'Your PDS did not respond' })
			})
		});
		await expect.element(page.getByText('Planner unavailable')).toBeVisible();
		await expect.element(page.getByText('Your PDS did not respond')).toBeVisible();
	});
});
