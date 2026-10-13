import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { DEMO_DJ_PROFILE, DEMO_DJ_SETS } from '../../../../scripts/seed-data.ts';
import { djProfileSchema, djSetSchema } from '#lib/dj-schema.ts';
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
			details: null,
			managers: [],
			dj: Promise.resolve([]),
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

const neon: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/2',
	rkey: '2',
	cid: 'c',
	slug: 'neon-tide',
	profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
	managers: ['did:plc:neon'],
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
	project: neon,
	details: null,
	managers: [{ did: 'did:plc:neon', handle: 'neon.test' }],
	isManager: false,
	saved: null,
	pendingClaim: null,
	pendingUpdate: null
};

it("shows the managers' playing stats and sets once they arrive", async () => {
	const profile = djProfileSchema.parse(DEMO_DJ_PROFILE);

	const sets = DEMO_DJ_SETS.map((set, i) => ({
		...djSetSchema.parse(set),
		uri: `at://did:plc:neon/computer.localnetwork.dj.set/${i}`
	}));

	await render(Page, {
		params: { slug: 'neon-tide' },
		data: { ...base, dj: Promise.resolve([{ did: 'did:plc:neon', handle: 'neon.test', profile, sets }]) },
		form: null
	});

	await expect.element(page.getByRole('heading', { name: 'How they play' })).toBeInTheDocument();
	await expect.element(page.getByText(/126–134 BPM/)).toBeInTheDocument();
	await expect.element(page.getByRole('heading', { name: 'Recent sets' })).toBeInTheDocument();
	await expect.element(page.getByText('Halle Nord, Berlin · with Halflight · 5 tracks')).toBeInTheDocument();
	await expect
		.element(page.getByRole('link', { name: 'Press kit' }))
		.toHaveAttribute('href', '/projects/neon-tide/epk');
});

it('asks a manager with nothing on file to publish from locallibrary', async () => {
	await render(Page, {
		params: { slug: 'neon-tide' },
		data: { ...base, isManager: true, dj: Promise.resolve([]) },
		form: null
	});

	await expect.element(page.getByRole('heading', { name: 'Show how you play' })).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'locallibrary' })).toHaveAttribute('href', '/tools/locallibrary');
});
