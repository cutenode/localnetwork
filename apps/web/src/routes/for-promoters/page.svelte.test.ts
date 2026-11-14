import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { NEON_TIDE, PAT } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';
import type { Viewer } from '#lib/types.ts';
import type { PageData } from './$types';
import Page from './+page.svelte';

const found = { example: NEON_TIDE, djs: 12, cities: 10, unclaimed: 7 };

const shortlist = {
	genre: 'techno',
	place: 'Berlin, DE',
	href: '/match?go=1&genres=techno',
	results: [
		{
			candidate: NEON_TIDE,
			score: 88,
			breakdown: { genre: 100, distance: 100, budget: 50, capacity: 50, setType: 50, format: 50 },
			reasons: ['Plays techno', 'Local'],
			warnings: []
		}
	]
};

type State = Pick<PageData, 'directory' | 'shortlist' | 'promoting'>;

const live: State = {
	directory: Promise.resolve(found),
	shortlist: Promise.resolve(shortlist),
	promoting: Promise.resolve(false)
};

const show = (state: Partial<State> = {}, viewer: Viewer | null = null) =>
	render(Page, {
		params: {},
		form: null,
		data: { viewer, pendingRequests: 0, meta: PAGE_META, geocoder: false, ...live, ...state }
	});

const region = (name: string) => page.getByRole('region', { name });

describe('/for-promoters', () => {
	it('searches Find DJs from the top of the page', async () => {
		await show();

		const search = page.getByRole('search', { name: 'Find DJs' });
		await expect.element(search.getByLabelText('Sound or name')).toHaveAttribute('name', 'q');
		expect(search.element().getAttribute('action')).toBe('/projects');
		await expect.element(page.getByRole('link', { name: 'For DJs' })).toHaveAttribute('href', '/for-djs');
	});

	it("shows a real listing and the matcher's real shortlist, with what they came from", async () => {
		await show();

		const find = region('Search the way you book.');
		await expect.element(find.getByRole('link', { name: /^Neon Tide/ })).toHaveAttribute('href', '/projects/neon-tide');
		await expect.element(find.getByText(/12 DJs in 10 cities so far/)).toBeVisible();

		const match = region('Describe the night. Get a shortlist with reasons.');
		await expect
			.element(match.getByRole('list', { name: 'Shortlist' }).getByRole('link', { name: 'Neon Tide' }))
			.toHaveAttribute('href', '/projects/neon-tide');
		await expect.element(match.getByText('Plays techno')).toBeVisible();
		await expect.element(match.getByText("The matcher's answer right now for techno near Berlin, DE.")).toBeVisible();
		await expect
			.element(match.getByRole('link', { name: 'Match a show' }))
			.toHaveAttribute('href', '/match?go=1&genres=techno');
		await expect
			.element(page.getByRole('link', { name: "See Neon Tide's press kits" }))
			.toHaveAttribute('href', '/projects/neon-tide/press-kits');
	});

	it('marks the screens filled with made-up data as examples', async () => {
		await show();

		await expect.element(region('One private thread per booking.').getByText('Example')).toBeVisible();
		await expect.element(region('Shortlists and lineups only you can read.').getByText('Example')).toBeVisible();
		await expect.element(region('Search the way you book.').getByText('Example')).not.toBeInTheDocument();
	});

	it('asks someone signed out to sign in and turn on promoting', async () => {
		await show();

		await expect
			.element(region('Put on shows? Turn on promoting.').getByRole('link', { name: 'Sign in' }))
			.toHaveAttribute('href', '/login?next=%2Fdashboard%2Fprofile%23section-promoter');
	});

	it('takes someone signed in straight to the promoter section, or to their planner once it is on', async () => {
		const { rerender } = await show({}, PAT);
		await expect
			.element(page.getByRole('link', { name: 'Turn on promoting' }))
			.toHaveAttribute('href', '/dashboard/profile#section-promoter');

		await rerender({
			data: {
				viewer: PAT,
				pendingRequests: 0,
				meta: PAGE_META,
				geocoder: false,
				...live,
				promoting: Promise.resolve(true)
			}
		});
		await expect.element(page.getByRole('heading', { name: "You're set up to book." })).toBeVisible();
		await expect
			.element(page.getByRole('link', { name: 'Your planner' }))
			.toHaveAttribute('href', '/dashboard/planner');
	});

	it('holds a place for the listing and the shortlist while the directory answers', async () => {
		const waiting = new Promise<never>(() => {});
		await show({ directory: waiting, shortlist: waiting, promoting: waiting });

		await expect.element(region('Search the way you book.').getByRole('status')).toHaveTextContent('Loading a listing');
		await expect
			.poll(() => page.getByRole('list', { name: 'Shortlist' }).element().querySelectorAll('li').length)
			.toBe(3);
		await expect.element(page.getByRole('link', { name: 'Sign in' })).toBeVisible();
	});

	it('says when the directory is offline, or has nobody yet', async () => {
		const { rerender } = await show({ directory: Promise.resolve(null), shortlist: Promise.resolve(null) });
		await expect.element(page.getByText("The directory isn't answering. Try again in a minute.").first()).toBeVisible();
		await expect
			.element(page.getByRole('list', { name: 'Shortlist' }))
			.toHaveTextContent("The directory isn't answering. Try again in a minute.");

		await rerender({
			data: {
				viewer: null,
				pendingRequests: 0,
				meta: PAGE_META,
				geocoder: false,
				...live,
				directory: Promise.resolve({ example: null, djs: 0, cities: 0, unclaimed: 0 }),
				shortlist: Promise.resolve({ genre: null, place: null, href: '/match', results: [] })
			}
		});
		await expect.element(page.getByText('The first listing will show here.')).toBeVisible();
		await expect.element(page.getByText('The first matches will show here.')).toBeVisible();
	});
});
