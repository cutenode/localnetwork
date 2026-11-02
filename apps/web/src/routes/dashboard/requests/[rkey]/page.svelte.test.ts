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
	request: request('update'),
	project: neonTide,
	changes: [{ field: 'profile.tagline', after: 'Dub' }],
	avatarUrl: null,
	values: {},
	submitted: true,
	geocoder: false,
	sets: undefined
};

it('confirms a submission in plain words, without where the record is stored', async () => {
	await render(Page, { params: { rkey: '3kupdate' }, data: base, form: null });

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('Changes: Neon Tide');
	await expect.element(page.getByText('Submitted. A moderator will review it soon.')).toBeInTheDocument();
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
