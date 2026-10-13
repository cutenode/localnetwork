import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ProjectView } from '#lib/types.ts';
import Page from './+page.svelte';

const project: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/2',
	rkey: '2',
	cid: 'c',
	slug: 'neon-tide',
	profile: {
		name: 'Neon Tide',
		kind: 'solo',
		tagline: 'Rolling, hypnotic techno for late rooms',
		genres: ['techno', 'dub techno'],
		location: { city: 'Berlin', country: 'DE' },
		fee: { tier: 'established', currency: 'EUR', minAmount: 600, maxAmount: 1500 },
		mixes: [{ title: 'Tidal Hours 004', url: 'https://soundcloud.com/neon/tidal-hours-004' }],
		highlights: ['Four-year residency at Halle Nord']
	},
	managers: ['did:plc:neon'],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:a',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
};

const data = {
	viewer: null,
	pendingRequests: 0,
	project,
	details: null,
	managers: [{ did: 'did:plc:neon', handle: 'neon.test' }],
	isManager: false,
	dj: Promise.resolve([]),
	profileUrl: 'https://localnetwork.computer/projects/neon-tide'
};

it('lays out the press kit from the profile', async () => {
	await render(Page, { params: { slug: 'neon-tide' }, data, form: null });

	await expect.element(page.getByRole('heading', { level: 1, name: 'Neon Tide' })).toBeInTheDocument();
	await expect.element(page.getByText('Rolling, hypnotic techno for late rooms').first()).toBeInTheDocument();
	await expect.element(page.getByText('Established · €600–€1,500')).toBeInTheDocument();
	await expect
		.element(page.getByRole('link', { name: 'Tidal Hours 004' }))
		.toHaveAttribute('href', 'https://soundcloud.com/neon/tidal-hours-004');
	await expect.element(page.getByText('Four-year residency at Halle Nord')).toBeInTheDocument();
	await expect.element(page.getByRole('button', { name: 'Print or save as PDF' })).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Send inquiry' })).not.toBeInTheDocument();
	await expect.element(page.getByRole('heading', { name: 'Not in your press kit yet' })).not.toBeInTheDocument();
});

it('lets a signed-in member send an inquiry', async () => {
	const viewer = {
		did: 'did:plc:pat',
		handle: 'pat.test',
		roles: [],
		isAdmin: false,
		isStaff: false,
		spacesCapable: true
	};

	await render(Page, { params: { slug: 'neon-tide' }, data: { ...data, viewer }, form: null });

	await expect
		.element(page.getByRole('link', { name: 'Send inquiry' }))
		.toHaveAttribute('href', '/bookings/new?project=neon-tide');
});

it("shows a manager what the press kit can't hold yet", async () => {
	await render(Page, { params: { slug: 'neon-tide' }, data: { ...data, isManager: true }, form: null });

	await expect.element(page.getByRole('heading', { name: 'Not in your press kit yet' })).toBeVisible();
	await expect.element(page.getByText('Press photos')).toBeVisible();
	await expect.element(page.getByRole('link', { name: 'Send inquiry' })).not.toBeInTheDocument();
});
