import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { PAGE_META } from '#lib/testing/page-meta.ts';
import type { ProjectView } from '#lib/types.ts';
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

const base = { viewer: null, pendingRequests: 0, meta: PAGE_META };

const show = (directory: PageData['directory']) =>
	render(Page, { params: {}, form: null, data: { ...base, directory } });

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

		await expect.element(page.getByRole('status')).toHaveTextContent('Loading a listing');
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
