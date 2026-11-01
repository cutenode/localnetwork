import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ProfileView, RequestView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';

const DJ = 'did:plc:neon';

const staff: Viewer = {
	did: 'did:plc:mod',
	handle: 'mod.test',
	roles: ['moderator'],
	isAdmin: false,
	isStaff: true,
	spacesCapable: true
};

const request: RequestView = {
	uri: `at://${DJ}/computer.localnetwork.moderation.request/3req`,
	cid: 'bafyrequest',
	rkey: '3req',
	kind: 'list',
	status: 'pending',
	requester: { did: DJ, handle: 'neon.test' },
	repoDid: DJ,
	projectUri: null,
	project: null,
	record: {
		kind: 'list',
		listing: { subject: `at://${DJ}/computer.localnetwork.profile/self`, cid: 'bafyasked' },
		createdAt: '2026-10-30T00:00:00Z'
	},
	createdAt: '2026-10-30T00:00:00Z',
	updatedAt: '2026-10-30T00:00:00Z',
	decision: null,
	resultProjectSlug: null
};

const profile: ProfileView = {
	did: DJ,
	uri: `at://${DJ}/computer.localnetwork.profile/self`,
	cid: 'bafynow',
	record: {
		basics: { name: 'Neon', location: { city: 'Berlin', country: 'DE' } },
		performer: { tagline: 'Hypnotic techno', genres: ['techno'] },
		createdAt: '2026-10-30T00:00:00Z',
		updatedAt: '2026-10-30T00:00:00Z'
	},
	avatarUrl: null
};

it('shows staff the profile to list, and flags a change since the request and a solo project that would list them twice', async () => {
	await render(Page, {
		params: { rkey: '3req' },
		data: {
			viewer: staff,
			pendingRequests: 1,
			request,
			project: null,
			listing: { profile, changed: true, soloProjects: [{ slug: 'neon-tide', name: 'Neon Tide' }], listed: false },
			changes: [],
			detailChanges: [],
			history: [],
			trail: [],
			stale: false,
			verification: null,
			hasAvatar: false,
			tracklists: []
		},
		form: null
	});

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('Listing: @neon.test');
	await expect.element(page.getByText('Hypnotic techno')).toBeInTheDocument();
	await expect.element(page.getByText('techno · Berlin, DE')).toBeInTheDocument();
	await expect.element(page.getByText(/Their profile changed since they asked/)).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Neon Tide' })).toHaveAttribute('href', '/projects/neon-tide');
	await expect.element(page.getByRole('link', { name: 'Open their profile' })).toHaveAttribute('href', '/@neon.test');
	await expect.element(page.getByRole('radio', { name: 'Approve and list' })).toBeChecked();
});
