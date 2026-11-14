import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { PAGE_META } from '#lib/testing/page-meta.ts';
import { capture } from '#lib/testing/tools.ts';
import type { ProjectView, SupportedTrack } from '#lib/types.ts';
import type { PageData } from './$types';
import Page from './+page.svelte';

const example: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/1',
	rkey: '1',
	cid: 'c',
	slug: 'neon-tide',
	profile: {
		name: 'Neon Tide',
		kind: 'solo',
		tagline: 'Rolling techno',
		genres: ['techno'],
		location: { city: 'Berlin', country: 'DE' }
	},
	managers: ['did:plc:neon'],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:a',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
};

const tracks: SupportedTrack[] = [{ id: '1', title: 'Night Bus', artist: 'Ferrule', artists: [], supports: 9 }];

const base = {
	viewer: null,
	pendingRequests: 0,
	meta: PAGE_META,
	shows: Promise.resolve([]),
	tracks: Promise.resolve(tracks),
	captures: [{ line: 'Keeps drives in sync', capture: capture('The drives view') }]
};

const show = (directory: PageData['directory'], more: Partial<PageData> = {}) =>
	render(Page, { params: {}, form: null, data: { ...base, directory, ...more } });

const region = (name: string) => page.getByRole('region', { name });

describe('/for-djs', () => {
	it('shows a real listing beside what promoters search on, with the totals', async () => {
		await show(Promise.resolve({ example, djs: 12, cities: 10, unclaimed: 7 }));

		await expect.element(page.getByRole('link', { name: 'Neon Tide' })).toHaveAttribute('href', '/projects/neon-tide');
		await expect.element(page.getByText(/12 DJs in 10 cities so far/)).toBeVisible();
		await expect
			.element(page.getByRole('link', { name: '7 waiting to be claimed' }))
			.toHaveAttribute('href', '/projects?unclaimed=1');
		await expect.element(page.getByText(/within 50 km of their venue/)).toBeVisible();
	});

	it('holds a place for the card while the directory answers', async () => {
		await show(new Promise(() => {}));

		await expect
			.element(region('Built around how promoters book.').getByRole('status'))
			.toHaveTextContent('Loading a listing');
	});

	it('starts a DJ on the DJ section of their profile, or a claim', async () => {
		await show(Promise.resolve({ example, djs: 1, cities: 1, unclaimed: 0 }));

		const hero = region('Get booked by promoters near you.');
		await expect
			.element(hero.getByRole('link', { name: 'Get listed' }))
			.toHaveAttribute('href', '/dashboard/profile#section-performer');
		await expect
			.element(hero.getByRole('link', { name: 'Find and claim yours' }))
			.toHaveAttribute('href', '/projects?unclaimed=1');
		await expect.element(region('How listing works.').getByText('Ask to be listed')).toBeVisible();
	});

	it('shows press kits, locallibrary, the charts and shows with real examples', async () => {
		await show(Promise.resolve({ example, djs: 1, cities: 1, unclaimed: 0 }));

		await expect
			.element(page.getByRole('link', { name: "See Neon Tide's press kits" }))
			.toHaveAttribute('href', '/projects/neon-tide/press-kits');
		await expect
			.element(region('locallibrary, from the crate to the booth.').getByRole('img', { name: 'The drives view' }))
			.toBeVisible();
		await expect.element(page.getByText('Keeps drives in sync.')).toBeVisible();
		await expect
			.element(page.getByRole('list', { name: 'Most played this month' }).getByText('Night Bus'))
			.toBeVisible();
		await expect.element(region('Announce your shows.').getByText('No shows announced yet')).toBeVisible();
		await expect.element(page.getByText('Coming soon: events and RSVPs')).toBeVisible();
	});

	it('says when the charts or the shows are offline', async () => {
		await show(Promise.resolve(null), { shows: Promise.resolve(null), tracks: Promise.resolve(null) });

		await expect.element(page.getByText("The charts aren't answering. Try again in a minute.")).toBeVisible();
		await expect
			.element(region('Announce your shows.').getByRole('link', { name: /Try again/ }))
			.toHaveAttribute('href', '/for-djs');
	});

	it('says when the directory is offline, or has nobody yet', async () => {
		const { rerender } = await show(Promise.resolve(null));
		await expect.element(page.getByText("The directory isn't answering. Try again in a minute.")).toBeVisible();

		await rerender({
			data: { ...base, directory: Promise.resolve({ example: null, djs: 0, cities: 0, unclaimed: 0 }) }
		});
		await expect.element(page.getByText('The first listing will show here.')).toBeVisible();
	});
});
