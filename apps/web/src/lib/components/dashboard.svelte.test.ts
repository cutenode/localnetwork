import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { RequestView, Viewer } from '#lib/types.ts';
import Handle from './Handle.svelte';
import ManageLink from './ManageLink.svelte';
import ProfileSetupCard from './ProfileSetupCard.svelte';
import PreviewBanner from './PreviewBanner.svelte';
import RequestList from './RequestList.svelte';

const viewer = (did: string, staff = false): Viewer => ({
	did,
	handle: null,
	roles: staff ? ['moderator'] : [],
	isAdmin: false,
	isStaff: staff,
	spacesCapable: true
});

describe('ManageLink', () => {
	const project = { slug: 'neon-tide', managers: ['did:plc:neon'] };

	it("takes a manager to the project's dashboard, or to one of its pages", async () => {
		const { rerender } = await render(ManageLink, { viewer: viewer('did:plc:neon'), project });

		await expect
			.element(page.getByRole('link', { name: 'Manage', exact: true }))
			.toHaveAttribute('href', '/dashboard/projects/neon-tide');
		await rerender({ viewer: viewer('did:plc:neon', true), project, section: 'press-kits' });
		// Staff who also manage it go to the dashboard too.
		await expect
			.element(page.getByRole('link', { name: 'Manage', exact: true }))
			.toHaveAttribute('href', '/dashboard/projects/neon-tide/press-kits');
	});

	it('takes other staff to the staff console, and shows nobody else anything', async () => {
		const { rerender } = await render(ManageLink, { viewer: viewer('did:plc:mod', true), project });

		await expect
			.element(page.getByRole('link', { name: 'Manage', exact: true }))
			.toHaveAttribute('href', '/admin/projects/neon-tide');
		await rerender({ viewer: viewer('did:plc:fan'), project });
		await expect.element(page.getByRole('link', { name: 'Manage', exact: true })).not.toBeInTheDocument();
		await rerender({ viewer: null, project });
		await expect.element(page.getByRole('link', { name: 'Manage', exact: true })).not.toBeInTheDocument();
	});

	it("takes a profile's owner to their profile editor, staff to what the account did, and nobody else anywhere", async () => {
		const profile = { did: 'did:plc:neon' };
		const { rerender } = await render(ManageLink, { viewer: viewer('did:plc:neon'), profile });
		const link = page.getByRole('link', { name: 'Manage', exact: true });

		await expect.element(link).toHaveAttribute('href', '/dashboard/profile');
		await rerender({ viewer: viewer('did:plc:mod', true), profile });
		await expect.element(link).toHaveAttribute('href', '/admin/audit?actor=did%3Aplc%3Aneon');
		await rerender({ viewer: viewer('did:plc:fan'), profile });
		await expect.element(link).not.toBeInTheDocument();
	});
});

describe('Handle', () => {
	it("links an account to its profile, at its handle or else its DID, unless it's inside another link", async () => {
		const { rerender } = await render(Handle, { account: { did: 'did:plc:neon', handle: 'neon.test' } });

		await expect.element(page.getByRole('link', { name: '@neon.test' })).toHaveAttribute('href', '/@neon.test');
		await rerender({ account: { did: 'did:plc:neon', handle: null } });
		await expect.element(page.getByRole('link')).toHaveAttribute('href', '/@did:plc:neon');
		await rerender({ account: { did: 'did:plc:neon', handle: 'neon.test' }, link: false });
		await expect.element(page.getByText('@neon.test')).toBeInTheDocument();
		await expect.element(page.getByRole('link')).not.toBeInTheDocument();
	});
});

describe('PreviewBanner', () => {
	it("says when visitors can't see a project, and nothing when they can", async () => {
		const { rerender } = await render(PreviewBanner, { status: 'hidden' });

		await expect.element(page.getByText('Not public yet.')).toBeInTheDocument();
		await expect.element(page.getByText(/Only its managers and staff can see this page/)).toBeInTheDocument();
		await rerender({ status: 'archived' });
		await expect.element(page.getByText('Archived.')).toBeInTheDocument();
		await rerender({ status: 'active' });
		await expect.element(page.getByText(/can see this page/)).not.toBeInTheDocument();
	});
});

describe('RequestList', () => {
	const request = (rkey: string, fields: Partial<RequestView>): RequestView => ({
		uri: `at://did:plc:pat/computer.localnetwork.moderation.request/${rkey}`,
		cid: 'c',
		rkey,
		kind: 'claim',
		status: 'pending',
		requester: { did: 'did:plc:pat', handle: 'pat.test' },
		repoDid: 'did:plc:pat',
		projectUri: null,
		project: null,
		record: { kind: 'claim', createdAt: '2026-10-01T00:00:00Z' },
		createdAt: '2026-10-01T00:00:00Z',
		updatedAt: '2026-10-01T00:00:00Z',
		decision: null,
		resultProjectSlug: null,
		...fields
	});

	it("names each request by its kind and project, linking to where it's followed", async () => {
		await render(RequestList, {
			requests: [
				request('a', { project: { slug: 'halflight', name: 'Halflight' } }),
				request('b', {
					kind: 'create',
					status: 'approved',
					record: {
						kind: 'create',
						profile: { name: 'Night Shift', kind: 'solo', genres: [], location: { city: 'Leeds', country: 'GB' } },
						createdAt: '2026-10-01T00:00:00Z'
					}
				})
			]
		});

		await expect
			.element(page.getByRole('link', { name: 'Claim: Halflight' }))
			.toHaveAttribute('href', '/dashboard/requests/a');
		await expect
			.element(page.getByRole('link', { name: 'New project: Night Shift' }))
			.toHaveAttribute('href', '/dashboard/requests/b');
		await expect.element(page.getByText('Approved')).toBeInTheDocument();
	});

	it('says when there are none', async () => {
		await render(RequestList, { requests: [] });

		await expect.element(page.getByText('No requests yet.')).toBeInTheDocument();
	});
});

describe('ProfileSetupCard', () => {
	it('starts a profile from the Bluesky name, offering its picture only when there is one', async () => {
		const { rerender } = await render(ProfileSetupCard, { name: 'Neon', hasAvatar: true });

		await expect.element(page.getByLabelText('Name')).toHaveValue('Neon');
		await expect.element(page.getByLabelText('Use my Bluesky picture')).toBeChecked();
		expect(document.querySelector('form')?.getAttribute('action')).toBe('/dashboard/profile?/create');
		await expect
			.element(page.getByRole('link', { name: 'Fill in everything' }))
			.toHaveAttribute('href', '/dashboard/profile');
		await rerender({ name: 'neon.test', hasAvatar: false });
		await expect.element(page.getByLabelText('Use my Bluesky picture')).not.toBeInTheDocument();
	});
});
