import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { DjActivity } from '#lib/dj.ts';
import { NEON, NEON_CREW, neonPlaying, neonProfile as view } from '#lib/testing/profiles.ts';
import type { FollowState, ProfileView, ProjectView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';
import { PAGE_META } from '#lib/testing/page-meta.ts';

const withMixes = view({
	performer: { mixes: [{ title: 'Warehouse Hours', url: 'https://soundcloud.com/neon/warehouse' }] }
});

type Streamed = { dj?: DjActivity[]; playsIn?: ProjectView[]; follow?: Promise<FollowState | null> };

/**
 * The page's data for `profile` (null for an account without one, shown as `name`), as `viewer`, with what streams
 * in: their DJ records, the projects they play in, and their followers when they can be followed.
 */
const pageData = (
	profile: ProfileView | null,
	viewer: Viewer | null = null,
	name = profile?.record.basics.name ?? '',
	{ dj = [], playsIn = [NEON_CREW], follow }: Streamed = {}
) => ({
	viewer,
	pendingRequests: 0,
	profile,
	name,
	account: { did: NEON, handle: 'neon.test' },
	meta: PAGE_META,
	dj: Promise.resolve(dj),
	sets: undefined,
	playsIn: Promise.resolve(playsIn),
	follow
});

/** Renders the page with pageData's arguments, and the result of a form posted to it. */
const open = (
	profile: ProfileView | null,
	viewer?: Viewer | null,
	name?: string,
	streamed?: Streamed,
	form: { error: string; values: undefined; reauth: boolean } | null = null
) => render(Page, { params: { handle: 'neon.test' }, data: pageData(profile, viewer, name, streamed), form });

const fan: Viewer = {
	did: 'did:plc:fan',
	handle: 'fan.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

it("shows a DJ's details, with the projects they play in a tab away", async () => {
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
	await expect.element(page.getByRole('tab', { name: 'Plays in' })).toHaveAttribute('href', '?tab=plays-in');
	await expect.element(page.getByRole('link', { name: 'Manage' })).not.toBeInTheDocument();
});

it('leaves out sections that are off or empty, and offers its owner a way to change it', async () => {
	await open(view({ performer: {} }), {
		did: NEON,
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

it('shows a profile with one section as that section, without tabs', async () => {
	await open(view({ basics: { name: 'Neon Tide', bio: 'Warehouse techno.' } }), null, undefined, { playsIn: [] });

	await expect.element(page.getByRole('heading', { name: 'About' })).toBeInTheDocument();
	await expect.element(page.getByText('Warehouse techno.')).toBeInTheDocument();
	await expect.element(page.getByRole('tablist')).not.toBeInTheDocument();
});

it("shows what they're looking for and their highlights under About", async () => {
	await open(
		view({ performer: { lookingFor: 'Sunday closing slots', highlights: ['Four-year residency at Halle Nord'] } }),
		null,
		undefined,
		{
			playsIn: []
		}
	);

	await expect.element(page.getByRole('heading', { name: 'About' })).toBeInTheDocument();
	await expect.element(page.getByText('Sunday closing slots')).toBeInTheDocument();
	await expect.element(page.getByRole('listitem').getByText('Four-year residency at Halle Nord')).toBeInTheDocument();
});

it('says when a profile has nothing to show yet, and shows nothing more for an account without one', async () => {
	const { unmount } = await open(view({ performer: {} }), null, undefined, { playsIn: [] });

	await expect.element(page.getByText('Nothing on this profile yet')).toBeInTheDocument();
	await unmount();

	await open(null, null, 'Neon', { playsIn: [] });
	await expect.element(page.getByText("Hasn't set up a profile yet.")).toBeInTheDocument();
	await expect.element(page.getByText('Nothing on this profile yet')).not.toBeInTheDocument();
});

it('gives each section with something in it a tab, opening the first', async () => {
	await open(withMixes, null, undefined, { dj: neonPlaying() });

	await expect.element(page.getByRole('tab', { name: 'Plays in' })).toBeInTheDocument();
	expect(
		page
			.getByRole('tab')
			.elements()
			.map((tab) => tab.textContent)
	).toEqual(['Mixes', 'Sets', 'Plays in']);
	await expect.element(page.getByRole('tab', { name: 'Mixes' })).toHaveAttribute('aria-selected', 'true');
	await expect
		.element(page.getByRole('tabpanel', { name: 'Mixes' }).getByText('Warehouse Hours', { exact: true }))
		.toBeInTheDocument();
	await expect.element(page.getByRole('tab', { name: 'Sets' })).toHaveAttribute('href', '?tab=sets');
});

it('shows the follower count, and links to sign-in to follow when signed out', async () => {
	const profile = view({});

	const { rerender } = await open(profile, null, undefined, {
		follow: Promise.resolve({ followers: 3, following: false })
	});

	await expect
		.element(page.getByRole('link', { name: 'Follow 3 followers' }))
		.toHaveAttribute('href', '/login?next=%2F%40neon.test%23follow');

	await rerender({
		data: pageData(profile, fan, undefined, { follow: Promise.resolve({ followers: 4, following: true }) })
	});
	await expect
		.element(page.getByRole('button', { name: 'Following 4 followers' }))
		.toHaveAttribute('aria-pressed', 'true');
	await rerender({
		data: pageData(profile, fan, undefined, { follow: Promise.resolve({ followers: 3, following: false }) })
	});
	await expect
		.element(page.getByRole('button', { name: 'Follow 3 followers' }))
		.toHaveAttribute('aria-pressed', 'false');
});

it('offers no Follow to an account without a profile', async () => {
	await open(null, null, '@neon.test');

	await expect.element(page.getByRole('heading', { level: 1, name: '@neon.test' })).toBeInTheDocument();
	await expect.element(page.getByText(/^Follow/)).not.toBeInTheDocument();
});

it('offers to sign in again when following needs a permission the session lacks', async () => {
	await open(
		view({}),
		fan,
		undefined,
		{ follow: Promise.resolve({ followers: 0, following: false }) },
		{
			error: 'Sign in again to follow DJs',
			values: undefined,
			reauth: true
		}
	);

	await expect.element(page.getByText('Sign in again to follow DJs.', { exact: false })).toBeInTheDocument();
	const again = page.getByRole('button', { name: 'Sign in again' });
	expect(new FormData(again.element().closest('form')!).get('handle')).toBe('did:plc:fan');
});

it("says why following didn't work", async () => {
	await open(
		view({}),
		fan,
		undefined,
		{ follow: Promise.resolve({ followers: 0, following: false }) },
		{
			error: "Following isn't available right now. Try again in a minute.",
			values: undefined,
			reauth: false
		}
	);

	await expect
		.element(page.getByRole('alert'))
		.toHaveTextContent("Following isn't available right now. Try again in a minute.");
});
