import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ProjectView } from '#lib/types.ts';
import type { PageData } from './$types';
import Page from './+page.svelte';

const project: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/1',
	rkey: '1',
	cid: 'c',
	slug: 'neon-tide',
	profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
	managers: [],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:a',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
};

const base = {
	viewer: null,
	pendingRequests: 0,
	geocoder: false,
	tools: [
		{
			id: 'localthing',
			name: 'localthing',
			kind: 'Chrome extension',
			tagline: 'Does a thing.',
			features: ['Keeps drives in sync']
		}
	]
};

const directory = {
	recent: [project],
	stats: { active: 12, cities: 10, unclaimed: 3 },
	genres: [{ value: 'techno', count: 2 }],
	cities: [{ city: 'Berlin', country: 'DE', count: 2 }]
};

const show = (data: PageData) => render(Page, { params: {}, form: null, data });

describe('/', () => {
	it('searches the directory, and lists new projects, sounds, cities and tools', async () => {
		await show({ ...base, ...directory });
		await expect
			.element(page.getByRole('heading', { level: 1 }))
			.toHaveTextContent('The home for local DJs and the nights they play.');
		await expect.element(page.getByRole('search', { name: 'Find DJs' })).toHaveAttribute('action', '/projects');
		await expect.element(page.getByRole('textbox', { name: 'Sound or name' })).toHaveAttribute('name', 'q');
		await expect.element(page.getByRole('link', { name: 'Neon Tide' })).toHaveAttribute('href', '/projects/neon-tide');
		await expect
			.element(page.getByRole('link', { name: '3 up for claiming' }))
			.toHaveAttribute('href', '/projects?unclaimed=on');
		await expect
			.element(page.getByRole('link', { name: 'techno 2' }))
			.toHaveAttribute('href', '/projects?genres=techno');
		await expect
			.element(page.getByRole('link', { name: 'Berlin, DE 2' }))
			.toHaveAttribute('href', '/projects?q=Berlin&country=DE');
		await expect.element(page.getByRole('link', { name: /localthing/ })).toHaveAttribute('href', '/tools/localthing');
	});

	it('says so when nothing is listed yet', async () => {
		await show({ ...base, recent: [], stats: { active: 0, cities: 0, unclaimed: 0 }, genres: [], cities: [] });
		await expect.element(page.getByText('No projects listed yet.')).toBeInTheDocument();
		await expect.element(page.getByRole('heading', { name: 'Find DJs by sound or city' })).not.toBeInTheDocument();
	});

	it('keeps the page up when the directory is down', async () => {
		await show({ ...base, recent: [], stats: null, genres: [], cities: [] });
		await expect.element(page.getByText("The directory isn't answering right now.")).toBeInTheDocument();
		await expect.element(page.getByRole('search', { name: 'Find DJs' })).toBeInTheDocument();
	});
});
