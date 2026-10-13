import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ProjectView } from '#lib/types.ts';
import Page from './+page.svelte';

it('renders a record that repeats genres, highlights, links, and mixes', async () => {
	const mix = { title: 'Tidal Hours', url: 'https://soundcloud.com/neon/tidal-hours' };
	const link = { kind: 'website', url: 'https://neon.example/' };

	const project: ProjectView = {
		uri: 'at://did:plc:root/computer.localnetwork.project/1',
		rkey: '1',
		cid: 'c',
		slug: 'neon-tide',
		profile: {
			name: 'Neon Tide',
			kind: 'solo',
			genres: ['techno', 'techno'],
			location: { city: 'Berlin', country: 'DE' },
			highlights: ['Residency', 'Residency'],
			links: [link, { ...link }],
			mixes: [mix, { ...mix }]
		},
		managers: [],
		status: 'active',
		revision: 1,
		createdBy: 'did:plc:a',
		createdAt: '2026-01-01T00:00:00Z',
		updatedAt: '2026-01-01T00:00:00Z',
		avatarUrl: null
	};

	await render(Page, {
		params: { slug: project.slug },
		data: {
			viewer: null,
			pendingRequests: 0,
			project,
			sets: undefined,
			details: null,
			managers: [],
			isManager: false,
			saved: null,
			pendingClaim: null,
			pendingUpdate: null
		},
		form: null
	});
	await expect.element(page.getByRole('link', { name: 'techno' }).nth(1)).toBeInTheDocument();
	await expect.element(page.getByText('Residency').nth(1)).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Website' }).nth(1)).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Tidal Hours' }).nth(1)).toBeInTheDocument();
});
