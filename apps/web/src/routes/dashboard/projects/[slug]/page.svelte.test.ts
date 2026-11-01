import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { EVENT_STATUS } from '#lib/shows.ts';
import type { ProjectView, RequestView, ShowView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';

const neon: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/2',
	rkey: '2',
	cid: 'c',
	slug: 'neon-tide',
	profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
	managers: ['did:plc:neon', 'did:plc:co'],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:a',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
};

const viewer: Viewer = {
	did: 'did:plc:neon',
	handle: 'neon.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

const show = (did: string, rkey: string, name: string): ShowView => ({
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
		createdAt: '2026-10-01T00:00:00Z'
	}
});

const base = {
	viewer,
	pendingRequests: 0,
	unread: 0,
	projects: Promise.resolve([neon]),
	sections: Promise.resolve(null),
	project: neon,
	managers: [
		{ did: 'did:plc:neon', handle: 'neon.test' },
		{ did: 'did:plc:co', handle: 'co.test' }
	],
	pendingUpdate: null,
	shows: Promise.resolve([]),
	edited: Promise.resolve([])
};

it("lists the project's shows, with edit links on the viewer's own and a note on ones another app changed", async () => {
	const own = show('did:plc:neon', 'a', 'Tidal Hours');
	const theirs = show('did:plc:co', 'b', 'Closing Party');

	await render(Page, {
		params: { slug: 'neon-tide' },
		data: { ...base, shows: Promise.resolve([own, theirs]), edited: Promise.resolve([own.uri]) },
		form: null
	});

	await expect
		.element(page.getByRole('link', { name: 'Edit Tidal Hours' }))
		.toHaveAttribute('href', '/dashboard/projects/neon-tide/shows/a');
	await expect.element(page.getByRole('link', { name: 'Edit Closing Party' })).not.toBeInTheDocument();
	await expect.element(page.getByText(/Changed in another calendar app/)).toBeInTheDocument();
	await expect
		.element(page.getByRole('link', { name: 'Announce a show' }))
		.toHaveAttribute('href', '/dashboard/projects/neon-tide/shows/new');
});

it("links to the project's sections, its public page, and how to publish stats", async () => {
	await render(Page, { params: { slug: 'neon-tide' }, data: base, form: null });

	await expect
		.element(page.getByRole('link', { name: 'Propose changes' }))
		.toHaveAttribute('href', '/dashboard/projects/neon-tide/profile');
	await expect
		.element(page.getByRole('link', { name: 'Press kits' }))
		.toHaveAttribute('href', '/dashboard/projects/neon-tide/press-kits');
	await expect
		.element(page.getByRole('link', { name: 'View public page' }))
		.toHaveAttribute('href', '/projects/neon-tide');
	await expect.element(page.getByRole('link', { name: 'locallibrary' })).toHaveAttribute('href', '/tools/locallibrary');
	await expect.element(page.getByText('@co.test')).toBeInTheDocument();
	await expect.element(page.getByText('Public. Anyone can find it.')).toBeInTheDocument();
	await expect.element(page.getByText('No upcoming shows')).toBeInTheDocument();
});

it('sends a manager with changes waiting to their request, and holds shows for a hidden project', async () => {
	// SAFETY: the page reads only the open request's key.
	const pendingUpdate = { rkey: '3kreq' } as RequestView;

	await render(Page, {
		params: { slug: 'neon-tide' },
		data: { ...base, project: { ...neon, status: 'hidden' }, pendingUpdate },
		form: null
	});

	await expect
		.element(page.getByRole('link', { name: 'View request' }))
		.toHaveAttribute('href', '/dashboard/requests/3kreq');
	await expect.element(page.getByRole('link', { name: 'Propose changes' })).not.toBeInTheDocument();
	await expect.element(page.getByText('Not public yet. Only its managers and staff can see it.')).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Announce a show' })).not.toBeInTheDocument();
});
