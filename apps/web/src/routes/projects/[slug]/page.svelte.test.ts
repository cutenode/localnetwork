import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { DEMO_DJ_PROFILE, DEMO_DJ_SETS } from '../../../../scripts/seed-data.ts';
import { djProfileSchema, djSetSchema } from '#lib/dj-schema.ts';
import { EVENT_STATUS } from '#lib/shows.ts';
import type { ProjectView, ShowView, Viewer } from '#lib/types.ts';
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
			sets: undefined,
			details: null,
			managers: [],
			dj: Promise.resolve([]),
			isManager: false,
			saved: null,
			pendingClaim: null,
			pendingUpdate: null,
			shows: Promise.resolve([]),
			follow: Promise.resolve({ followers: 0, following: false })
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
	sets: undefined,
	details: null,
	managers: [{ did: 'did:plc:neon', handle: 'neon.test' }],
	isManager: false,
	saved: null,
	pendingClaim: null,
	pendingUpdate: null,
	shows: Promise.resolve([]),
	follow: Promise.resolve({ followers: 0, following: false }),
	dj: Promise.resolve([])
};

const neonViewer: Viewer = {
	did: 'did:plc:neon',
	handle: 'neon.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

const show = (did: string, rkey: string, name: string, fields: Partial<ShowView['record']> = {}): ShowView => ({
	uri: `at://${did}/computer.localnetwork.show/${rkey}`,
	cid: 'c',
	did,
	rkey,
	project: neon,
	record: {
		project: neon.uri,
		event: { uri: `at://${did}/community.lexicon.calendar.event/${rkey}`, cid: 'e' },
		name,
		startsAt: '2026-11-07T23:00:00+01:00',
		endsAt: '2026-11-08T06:00:00+01:00',
		venue: 'Halle Nord',
		location: { city: 'Berlin', country: 'DE' },
		status: EVENT_STATUS.scheduled,
		tickets: 'https://tickets.example/tidal',
		createdAt: '2026-10-01T00:00:00Z',
		...fields
	}
});

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
		.element(page.getByRole('link', { name: 'Press kits' }))
		.toHaveAttribute('href', '/projects/neon-tide/press-kits');
});

it('shows managers what visitors see: no edit links, no announce button, no nudges', async () => {
	const own = show('did:plc:neon', 'a', 'Tidal Hours');
	const theirs = show('did:plc:co', 'b', 'Closing Party', { status: EVENT_STATUS.cancelled, tickets: undefined });

	await render(Page, {
		params: { slug: 'neon-tide' },
		data: { ...base, viewer: neonViewer, isManager: true, shows: Promise.resolve([own, theirs]) },
		form: null
	});

	await expect.element(page.getByRole('heading', { name: 'Upcoming shows' })).toBeInTheDocument();
	await expect.element(page.getByText('23:00 to 06:00').first()).toBeInTheDocument();
	await expect.element(page.getByText('Cancelled')).toBeInTheDocument();
	// These are the page's own project's shows, so they don't link back to it.
	await expect.element(page.getByRole('link', { name: 'Neon Tide' })).not.toBeInTheDocument();
	await expect
		.element(page.getByRole('link', { name: 'Tickets' }))
		.toHaveAttribute('href', 'https://tickets.example/tidal');
	await expect.element(page.getByRole('link', { name: 'Edit Tidal Hours' })).not.toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Announce a show' })).not.toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Propose changes' })).not.toBeInTheDocument();
	await expect.element(page.getByRole('heading', { name: 'Show how you play' })).not.toBeInTheDocument();
	await expect
		.element(page.getByRole('link', { name: 'Manage', exact: true }))
		.toHaveAttribute('href', '/dashboard/projects/neon-tide');
});

it("links staff who don't manage it to the staff console, and nobody else", async () => {
	const moderator: Viewer = { ...neonViewer, did: 'did:plc:mod', roles: ['moderator'], isStaff: true };

	const { rerender } = await render(Page, {
		params: { slug: 'neon-tide' },
		data: { ...base, viewer: moderator },
		form: null
	});

	await expect
		.element(page.getByRole('link', { name: 'Manage', exact: true }))
		.toHaveAttribute('href', '/admin/projects/neon-tide');

	await rerender({ data: { ...base, viewer: { ...neonViewer, did: 'did:plc:pat' } } });
	await expect
		.element(page.getByRole('link', { name: 'Send inquiry' }))
		.toHaveAttribute('href', '/dashboard/bookings/new?project=neon-tide');
	await expect
		.element(page.getByRole('link', { name: 'Request to co-manage' }))
		.toHaveAttribute('href', '/dashboard/claim/neon-tide');
	await expect.element(page.getByRole('link', { name: 'Manage', exact: true })).not.toBeInTheDocument();

	await rerender({ data: base });
	await expect.element(page.getByRole('link', { name: 'Sign in to book' })).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Manage', exact: true })).not.toBeInTheDocument();
});

it("tells managers and staff when visitors can't see the page yet", async () => {
	const { rerender } = await render(Page, {
		params: { slug: 'neon-tide' },
		data: { ...base, viewer: neonViewer, isManager: true, project: { ...neon, status: 'hidden' } },
		form: null
	});

	await expect.element(page.getByText('Not public yet.')).toBeInTheDocument();
	await expect.element(page.getByText(/Only its managers and staff can see this page/)).toBeInTheDocument();

	await rerender({ data: { ...base, viewer: neonViewer, isManager: true } });
	await expect.element(page.getByText('Not public yet.')).not.toBeInTheDocument();
});

it('says when there are no shows, and that following brings new ones', async () => {
	await render(Page, { params: { slug: 'neon-tide' }, data: base, form: null });

	await expect.element(page.getByText('No upcoming shows')).toBeInTheDocument();
	await expect.element(page.getByText('Follow Neon Tide to see new ones under Shows.')).toBeInTheDocument();
});

it('shows the follower count, and links to sign-in to follow when signed out', async () => {
	const { rerender } = await render(Page, {
		params: { slug: 'neon-tide' },
		data: { ...base, follow: Promise.resolve({ followers: 3, following: false }) },
		form: null
	});

	await expect
		.element(page.getByRole('link', { name: 'Follow 3 followers' }))
		.toHaveAttribute('href', '/login?next=/projects/neon-tide');

	await rerender({ data: { ...base, viewer: neonViewer, follow: Promise.resolve({ followers: 1, following: true }) } });
	await expect
		.element(page.getByRole('button', { name: 'Following 1 follower' }))
		.toHaveAttribute('aria-pressed', 'true');
});

it('offers to sign in again when following needs a permission the session lacks', async () => {
	await render(Page, {
		params: { slug: 'neon-tide' },
		data: { ...base, viewer: neonViewer },
		form: { error: 'Sign in again to follow DJs', values: undefined, reauth: true }
	});

	await expect.element(page.getByText('Sign in again to follow DJs.', { exact: false })).toBeInTheDocument();
	const again = page.getByRole('button', { name: 'Sign in again' });
	await expect.element(again).toBeInTheDocument();
	expect(again.element().closest('form')?.getAttribute('action')).toBe('/login');
	expect(new FormData(again.element().closest('form')!).get('handle')).toBe('did:plc:neon');
});
