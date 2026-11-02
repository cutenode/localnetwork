import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ProfileView, RequestView, Viewer } from '#lib/types.ts';
import { ada, harbour, neon, neonTide, request as projectRequest } from '#lib/testing/requests.ts';
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
			managers: [],
			decided: null,
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
	await expect.element(page.getByRole('radio', { name: 'Approve and list' })).not.toBeChecked();
});

const base = {
	viewer: ada,
	pendingRequests: 1,
	request: projectRequest('update'),
	project: neonTide,
	listing: null,
	changes: [{ field: 'profile.tagline', after: 'Dub' }],
	detailChanges: [],
	stale: false,
	managers: [neon],
	decided: null,
	history: [],
	trail: [],
	verification: null,
	hasAvatar: false,
	tracklists: []
};

it('starts a decision with no outcome picked, and lists the managers a claim would join', async () => {
	await render(Page, {
		params: { rkey: '3kclaim' },
		data: {
			...base,
			request: projectRequest('claim', {
				requester: harbour,
				record: {
					kind: 'claim',
					claim: { relationship: 'member', evidence: 'I play in it' },
					createdAt: '2026-10-01T00:00:00Z'
				}
			})
		},
		form: null
	});

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('Claim: Neon Tide');
	await expect.element(page.getByText(/Current managers:/).getByText('@neon.test')).toBeInTheDocument();
	await expect.element(page.getByRole('radio', { name: 'Approve and add as manager' })).not.toBeChecked();
	const submit = page.getByRole('button', { name: 'Submit decision' });
	await expect.element(submit).toBeDisabled();
	await page.getByRole('radio', { name: 'Reject' }).click();
	await expect.element(submit).toBeEnabled();
});

it('warns about a stale pending request and says what the last decision did', async () => {
	await render(Page, {
		params: { rkey: '3kupdate' },
		data: { ...base, stale: true, decided: 'Approved and published' },
		form: null
	});

	await expect.element(page.getByText(/The project changed since this request was made/)).toBeInTheDocument();
	await expect
		.element(page.getByText('Approved and published. This is the next oldest request waiting for review.'))
		.toBeInTheDocument();
	await expect.element(page.getByRole('heading', { name: 'Proposed changes' })).toBeInTheDocument();
});

it('shows an approved request as the changes it published', async () => {
	await render(Page, {
		params: { rkey: '3kupdate' },
		data: {
			...base,
			request: projectRequest('update', {
				status: 'approved',
				decision: { by: ada, at: '2026-10-02T00:00:00Z', reason: null }
			})
		},
		form: null
	});

	await expect.element(page.getByRole('heading', { name: 'Published changes' })).toBeInTheDocument();
	await expect.element(page.getByText('Dub')).toBeInTheDocument();
	await expect.element(page.getByRole('button', { name: 'Submit decision' })).not.toBeInTheDocument();
});
