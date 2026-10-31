import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ProjectView } from '#lib/types.ts';
import Page from './+page.svelte';

const project = (slug: string, name: string, managers: string[]): ProjectView => ({
	uri: `at://did:plc:root/computer.localnetwork.project/${slug}`,
	rkey: slug,
	cid: 'c',
	slug,
	profile: { name, kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
	managers,
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:a',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
});

const neon = project('neon-tide', 'Neon Tide', ['did:plc:neon']);

const unclaimed = project('static-bloom', 'Static Bloom', []);

const show = {
	rkey: '3kshow',
	details: { title: 'Rooftop Rave' },
	lineup: [
		{ project: neon.uri, status: 'considering' },
		{ project: unclaimed.uri, status: 'considering' }
	],
	createdAt: '2026-01-01T00:00:00Z'
};

const data = {
	viewer: null,
	pendingRequests: 0,
	unread: 0,
	projects: { [neon.uri]: neon, [unclaimed.uri]: unclaimed },
	saves: [],
	show,
	isNew: false
};

it('links each lineup project that can be contacted to an inquiry filled in from the show', async () => {
	await render(Page, { params: { rkey: '3kshow' }, data, form: null });

	await expect
		.element(page.getByRole('link', { name: 'Inquire with Neon Tide' }))
		.toHaveAttribute('href', '/dashboard/bookings/new?project=neon-tide&show=3kshow');
	await expect.element(page.getByRole('link', { name: 'Inquire with Static Bloom' })).not.toBeInTheDocument();
});
