import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ProfileRecord, ProfileView, ProjectView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';

const DID = 'did:plc:neon';

const view = (record: Partial<ProfileRecord>): ProfileView => ({
	did: DID,
	uri: `at://${DID}/computer.localnetwork.profile/self`,
	cid: 'bafyprofile',
	record: {
		basics: { name: 'Neon Tide' },
		createdAt: '2026-10-30T00:00:00Z',
		updatedAt: '2026-10-30T00:00:00Z',
		...record
	},
	avatarUrl: null
});

const crew: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/1',
	rkey: '1',
	cid: 'bafyproject',
	slug: 'neon-crew',
	profile: { name: 'Neon Crew', kind: 'duo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
	managers: [DID],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:root',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
};

/** Renders the page for `profile` (null for an account without one, shown as `name`), as `viewer`. */
const open = (profile: ProfileView | null, viewer: Viewer | null = null, name = profile?.record.basics.name ?? '') =>
	render(Page, {
		params: { handle: 'neon.test' },
		data: {
			viewer,
			pendingRequests: 0,
			profile,
			name,
			account: { did: DID, handle: 'neon.test' },
			canonical: 'https://localnetwork.test/@neon.test',
			dj: Promise.resolve([]),
			sets: undefined,
			playsIn: Promise.resolve([crew])
		},
		form: null
	});

it("shows a DJ's sections, the projects they play in, and its canonical address", async () => {
	await open(
		view({
			basics: {
				name: 'Neon Tide',
				bio: 'Warehouse techno.',
				pronouns: 'they/them',
				location: { city: 'Berlin', country: 'DE' },
				links: [{ kind: 'website', url: 'https://neon.example/' }]
			},
			performer: { tagline: 'Four hours of rolling techno', genres: ['techno'], setTypes: ['closing'] },
			promoter: { cities: [{ city: 'Leipzig', country: 'DE' }] }
		})
	);

	await expect.element(page.getByRole('heading', { level: 1, name: 'Neon Tide' })).toBeInTheDocument();
	await expect.element(page.getByText('Four hours of rolling techno')).toBeInTheDocument();
	await expect.element(page.getByText('@neon.test')).toBeInTheDocument();
	await expect.element(page.getByText('Berlin, DE')).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'techno' })).toHaveAttribute('href', '/projects?genres=techno');
	await expect.element(page.getByText('Closing')).toBeInTheDocument();
	await expect.element(page.getByText('Leipzig, DE')).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Website' })).toHaveAttribute('href', 'https://neon.example/');
	await expect.element(page.getByRole('link', { name: 'Neon Crew' })).toHaveAttribute('href', '/projects/neon-crew');
	expect(document.querySelector('link[rel="canonical"]')?.getAttribute('href')).toBe(
		'https://localnetwork.test/@neon.test'
	);
	await expect.element(page.getByRole('link', { name: 'Manage' })).not.toBeInTheDocument();
});

it('leaves out sections that are off or empty, and offers its owner a way to change it', async () => {
	await open(view({ performer: {} }), {
		did: DID,
		handle: 'neon.test',
		roles: [],
		isAdmin: false,
		isStaff: false,
		spacesCapable: true
	});

	await expect.element(page.getByRole('heading', { level: 1, name: 'Neon Tide' })).toBeInTheDocument();
	await expect.element(page.getByRole('heading', { name: 'At a glance' })).not.toBeInTheDocument();
	await expect.element(page.getByRole('heading', { name: 'About' })).not.toBeInTheDocument();
	await expect.element(page.getByRole('heading', { name: 'Books shows in' })).not.toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Manage' })).toHaveAttribute('href', '/dashboard/profile');
});

it("makes only web addresses clickable, since people write their profiles' links themselves", async () => {
	const evil = 'javascript:alert(document.domain)';

	await open(
		view({
			basics: {
				name: 'Neon Tide',
				links: [
					{ kind: 'website', url: evil },
					{ kind: 'bandcamp', url: 'https://neon.example/' }
				]
			},
			performer: { mixes: [{ title: 'Trap', url: evil }] }
		})
	);

	await expect.element(page.getByRole('link', { name: 'Bandcamp' })).toHaveAttribute('href', 'https://neon.example/');
	await expect.element(page.getByText('Trap')).toBeInTheDocument();
	expect(document.querySelectorAll('a[href^="javascript:"]')).toHaveLength(0);
});

it("gives an account without a profile its name, what it plays in, and a line saying it hasn't set one up", async () => {
	await open(null, null, 'Neon');

	await expect.element(page.getByRole('heading', { level: 1, name: 'Neon' })).toBeInTheDocument();
	await expect.element(page.getByText('@neon.test')).toBeInTheDocument();
	await expect.element(page.getByText("Hasn't set up a profile yet.")).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Neon Crew' })).toHaveAttribute('href', '/projects/neon-crew');
	await expect.element(page.getByRole('heading', { name: 'On the AT Protocol' })).not.toBeInTheDocument();
});
