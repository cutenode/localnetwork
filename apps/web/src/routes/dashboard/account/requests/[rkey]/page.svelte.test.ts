import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { ada, neon, neonTide, request } from '#lib/testing/requests.ts';
import Page from './+page.svelte';

const viewer = { ...ada, did: neon.did, handle: neon.handle, roles: [], isAdmin: false, isStaff: false };

const base = {
	viewer,
	pendingRequests: 0,
	projects: Promise.resolve([neonTide]),
	sections: Promise.resolve(null),
	unread: 0,
	waiting: 0,
	needs: [],
	access: Promise.resolve([]),
	request: request('update'),
	project: neonTide,
	changes: [{ field: 'profile.tagline', after: 'Dub' }],
	avatarUrl: null,
	values: {},
	submitted: true,
	geocoder: false,
	sets: undefined
};

it('confirms a sent request in plain words, without where the record is stored', async () => {
	await render(Page, { params: { rkey: '3kupdate' }, data: base, form: null });

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('Changes: Neon Tide');
	await expect.element(page.getByText('Sent to staff. Nothing needed from you.')).toBeInTheDocument();
	await expect.element(page.getByRole('heading', { name: 'Proposed changes' })).toBeInTheDocument();
	expect(document.body.textContent).not.toMatch(/at:\/\/|PDS|moderation space/);
});

it('shows an approved request as the changes it published', async () => {
	await render(Page, {
		params: { rkey: '3kupdate' },
		data: {
			...base,
			submitted: false,
			request: request('update', {
				status: 'approved',
				decision: { by: ada, at: '2026-10-02T00:00:00Z', reason: null }
			})
		},
		form: null
	});

	await expect.element(page.getByRole('heading', { name: 'Published changes' })).toBeInTheDocument();
	await expect.element(page.getByText('Dub')).toBeInTheDocument();
});

it('says when the requester deleted what a decided request said', async () => {
	const decided = {
		status: 'rejected' as const,
		decision: { by: ada, at: '2026-10-02T00:00:00Z', reason: 'Duplicate' }
	};

	const { rerender } = await render(Page, {
		params: { rkey: '3kupdate' },
		data: { ...base, submitted: false, request: request('update', decided) },
		form: null
	});

	await expect.element(page.getByText(/you deleted what it said/)).not.toBeInTheDocument();
	await rerender({
		data: { ...base, submitted: false, request: request('update', { ...decided, deletedAt: '2026-10-05T12:00:00Z' }) }
	});
	await expect.element(page.getByText(/you deleted what it said/)).toBeVisible();
});

it('lets a DJ sent back for changes send their listing request again once their profile is updated', async () => {
	const listing = request('list', {
		project: null,
		projectUri: null,
		status: 'changes_requested',
		record: {
			kind: 'list',
			listing: { subject: 'at://did:plc:neon/computer.localnetwork.profile/self', cid: 'c' },
			createdAt: '2026-10-01T00:00:00Z'
		},
		decision: { by: ada, at: '2026-10-02T00:00:00Z', reason: 'Add a photo' }
	});

	const { rerender } = await render(Page, {
		params: { rkey: '3klist' },
		data: { ...base, submitted: false, request: listing },
		form: null
	});

	await expect.element(page.getByRole('link', { name: 'Edit your profile' })).toBeInTheDocument();
	await expect.element(page.getByRole('textbox', { name: 'Note' })).toBeInTheDocument();
	await expect.element(page.getByRole('button', { name: "I've updated my profile" })).toBeInTheDocument();

	// While it waits for review there's nothing to send back.
	await rerender({ data: { ...base, submitted: false, request: { ...listing, status: 'pending', decision: null } } });
	await expect.element(page.getByRole('button', { name: "I've updated my profile" })).not.toBeInTheDocument();
});
