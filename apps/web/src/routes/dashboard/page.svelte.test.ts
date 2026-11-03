import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { BookingThreadView, PlannerShowView, ProjectView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';

const viewer = (did: string, handle: string): Viewer => ({
	did,
	handle,
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
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

const waiting: BookingThreadView = {
	space: 'at://did:plc:root/computer.localnetwork.space.booking/3k',
	skey: '3k',
	status: 'negotiating',
	title: 'Closing Night',
	showDate: null,
	project: { uri: neon.uri, slug: 'neon-tide', name: 'Neon Tide' },
	promoter: { did: 'did:plc:pat', handle: 'pat.test' },
	participants: [
		{ did: 'did:plc:pat', handle: 'pat.test', role: 'promoter' },
		{ did: 'did:plc:neon', handle: 'neon.test', role: 'manager' }
	],
	lastAuthor: 'did:plc:neon',
	lastActivityAt: '2026-10-01T00:00:00Z',
	createdAt: '2026-10-01T00:00:00Z'
};

const show: PlannerShowView = {
	rkey: 's1',
	details: { title: 'Winter Social', venue: 'Kantine' },
	lineup: [{ project: neon.uri }],
	createdAt: '2026-10-01T00:00:00Z'
};

const base = {
	pendingRequests: 0,
	unread: 0,
	sections: Promise.resolve(null),
	requests: [],
	threads: [],
	waiting: 0,
	firstRun: null
};

it("leads a promoter's overview with threads waiting on them and their planner, not projects", async () => {
	await render(Page, {
		params: {},
		form: null,
		data: {
			...base,
			viewer: viewer('did:plc:pat', 'pat.test'),
			projects: Promise.resolve([]),
			books: Promise.resolve(true),
			threads: [waiting],
			waiting: 1,
			planner: Promise.resolve({
				available: true as const,
				saves: [{ rkey: 'a', project: neon.uri, createdAt: '' }],
				shows: [show],
				projects: { [neon.uri]: neon }
			})
		}
	});

	await expect.element(page.getByRole('heading', { name: '@pat.test' })).toBeInTheDocument();
	await expect.element(page.getByText(/did:plc/)).not.toBeInTheDocument();
	await expect
		.element(page.getByRole('heading', { name: /Booking threads/ }))
		.toHaveTextContent('Booking threads 1 waiting on you');
	await expect.element(page.getByText('Needs your reply')).toBeInTheDocument();
	await expect
		.element(page.getByRole('link', { name: 'Winter Social' }))
		.toHaveAttribute('href', '/dashboard/planner/shows/s1');
	await expect
		.element(page.getByRole('link', { name: 'Neon Tide', exact: true }))
		.toHaveAttribute('href', '/projects/neon-tide');
	await expect.element(page.getByRole('heading', { name: 'Projects you manage' })).not.toBeInTheDocument();
	await expect.element(page.getByRole('heading', { name: 'Your submissions' })).not.toBeInTheDocument();
	await expect
		.element(page.getByRole('link', { name: 'List a project' }))
		.toHaveAttribute('href', '/dashboard/projects/new');
});

it("shows a promoter's empty planner with next steps", async () => {
	await render(Page, {
		params: {},
		form: null,
		data: {
			...base,
			viewer: viewer('did:plc:pat', 'pat.test'),
			projects: Promise.resolve([]),
			books: Promise.resolve(true),
			planner: Promise.resolve({ available: true as const, saves: [], shows: [], projects: {} })
		}
	});

	await expect.element(page.getByRole('link', { name: 'Find DJs' }).first()).toHaveAttribute('href', '/projects');
	await expect
		.element(page.getByRole('link', { name: 'Plan a show' }))
		.toHaveAttribute('href', '/dashboard/planner/shows/new');
});

it("leads a DJ's overview with their projects, then their threads and submissions", async () => {
	await render(Page, {
		params: {},
		form: null,
		data: {
			...base,
			viewer: viewer('did:plc:neon', 'neon.test'),
			projects: Promise.resolve([neon]),
			books: Promise.resolve(false),
			planner: Promise.resolve(null)
		}
	});

	await expect
		.element(page.getByRole('link', { name: 'Manage Neon Tide' }))
		.toHaveAttribute('href', '/dashboard/projects/neon-tide');
	await expect.element(page.getByText(/When a promoter sends an inquiry/)).toBeInTheDocument();
	await expect.element(page.getByText('No submissions yet')).toBeInTheDocument();
	await expect.element(page.getByRole('heading', { name: 'Planned shows' })).not.toBeInTheDocument();
});

it('keeps a DJ profile without projects on the DJ overview, and offers a first profile to set up', async () => {
	await render(Page, {
		params: {},
		form: null,
		data: {
			...base,
			viewer: viewer('did:plc:neon', 'neon.test'),
			projects: Promise.resolve([]),
			books: Promise.resolve(false),
			planner: Promise.resolve(null),
			firstRun: { name: 'Neon', hasAvatar: false }
		}
	});

	await expect.element(page.getByRole('heading', { name: 'Set up your profile' })).toBeInTheDocument();
	await expect.element(page.getByText("You don't manage any projects yet")).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Find yours' })).toHaveAttribute('href', '/projects');
	await expect.element(page.getByRole('heading', { name: 'Planned shows' })).not.toBeInTheDocument();
});
