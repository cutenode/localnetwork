import { page } from 'vitest/browser';
import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, render } from 'vitest-browser-svelte';
import type { ProfileView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';

const viewer: Viewer = {
	did: 'did:plc:neon',
	handle: 'neon.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

const profile: ProfileView = {
	did: viewer.did,
	uri: `at://${viewer.did}/computer.localnetwork.profile/self`,
	cid: 'bafyprofile',
	record: {
		basics: { name: 'Neon' },
		performer: { tagline: 'Hypnotic techno' },
		createdAt: '2026-10-30T00:00:00Z',
		updatedAt: '2026-10-30T00:00:00Z'
	},
	avatarUrl: null
};

type Listing = { listing?: { listedAt: string } | null; listRequest?: { rkey: string; status: 'pending' } | null };

/** Renders the editor for `saved` (null for someone without a profile yet), with where they are in Find DJs. */
const open = (saved: ProfileView | null, needsSignIn = false, { listing = null, listRequest = null }: Listing = {}) =>
	render(Page, {
		params: {},
		data: {
			viewer,
			pendingRequests: 0,
			unread: 0,
			projects: Promise.resolve([]),
			sections: Promise.resolve(null),
			profile: saved,
			values: saved ? { name: 'Neon', tagline: 'Hypnotic techno' } : { name: 'Neon' },
			sectionsOn: { performer: !!saved?.record.performer, promoter: !!saved?.record.promoter },
			needsSignIn,
			listing,
			listRequest,
			geocoder: false,
			sets: Promise.resolve([])
		},
		form: null
	});

afterEach(() => {
	vi.restoreAllMocks();
});

it("shows a section's fields while its box is ticked, and links to the profile", async () => {
	await open(profile);

	await expect.element(page.getByRole('link', { name: 'View profile' })).toHaveAttribute('href', '/@neon.test');
	await expect.element(page.getByLabelText('Tagline')).toHaveValue('Hypnotic techno');
	await expect.element(page.getByText('Where you book')).not.toBeInTheDocument();

	await page.getByLabelText('I put on shows').click();
	await expect.element(page.getByText('Where you book')).toBeInTheDocument();
	await page.getByLabelText('I play out as a DJ').click();
	await expect.element(page.getByLabelText('Tagline')).not.toBeInTheDocument();
});

it('asks before saving a profile with a section turned off, since that deletes what was in it', async () => {
	const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
	await open(profile);

	await page.getByLabelText('I play out as a DJ').click();
	await page.getByRole('button', { name: 'Save profile' }).click();
	expect(confirm).toHaveBeenCalledWith("Turning off your DJ section deletes what's in it. Save anyway?");
});

it("offers to sign in again first when the session can't save a profile, and starts someone new from scratch", async () => {
	await open(null, true);

	await expect.element(page.getByRole('heading', { name: 'Set up your profile' })).toBeInTheDocument();
	await expect.element(page.getByRole('button', { name: 'Sign in again' })).toBeInTheDocument();
	await expect.element(page.getByLabelText('Name', { exact: true })).toHaveValue('Neon');
	await expect.element(page.getByRole('link', { name: 'View profile' })).not.toBeInTheDocument();
});

it('lets a DJ ask to be listed in Find DJs, and then says where that stands', async () => {
	await open(profile);
	await expect.element(page.getByRole('button', { name: 'Ask to be listed' })).toBeInTheDocument();
	expect(document.querySelector('form[action="?/requestListing"]')).not.toBeNull();

	cleanup();
	await open(profile, false, { listRequest: { rkey: '3req', status: 'pending' } });
	await expect
		.element(page.getByRole('link', { name: 'See your request' }))
		.toHaveAttribute('href', '/dashboard/requests/3req');
	await expect.element(page.getByRole('button', { name: 'Ask to be listed' })).not.toBeInTheDocument();

	cleanup();
	await open(profile, false, { listing: { listedAt: '2026-10-30T00:00:00Z' } });
	await expect.element(page.getByText(/^Staff list you in Find DJs/)).toBeInTheDocument();
});
