import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { BookingThreadView, ProjectView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';

const viewer = (did: string): Viewer => ({
	did,
	handle: null,
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
});

const thread = (skey: string, status: string, lastAuthor: string, project = 'neon-tide'): BookingThreadView => ({
	space: `at://did:plc:root/computer.localnetwork.space.booking/${skey}`,
	skey,
	status,
	title: `Show ${skey}`,
	showDate: null,
	project: {
		uri: `at://did:plc:root/computer.localnetwork.project/${project}`,
		slug: project,
		name: project === 'neon-tide' ? 'Neon Tide' : 'Harbour Lights'
	},
	promoter: { did: 'did:plc:pat', handle: 'pat.test' },
	participants: [
		{ did: 'did:plc:pat', handle: 'pat.test', role: 'promoter' },
		{ did: 'did:plc:neon', handle: 'neon.test', role: 'manager' }
	],
	lastAuthor,
	lastActivityAt: '2026-10-01T00:00:00Z',
	createdAt: '2026-10-01T00:00:00Z'
});

const data = (did: string, threads: BookingThreadView[], projects: ProjectView[] = []) => ({
	viewer: viewer(did),
	pendingRequests: 0,
	unread: 0,
	waiting: 0,
	needs: [],
	access: Promise.resolve([]),
	sections: Promise.resolve(null),
	projects: Promise.resolve(projects),
	threads
});

it('groups threads by whose turn it is and how they ended, with a filter once there are several projects', async () => {
	await render(Page, {
		params: {},
		form: null,
		data: data('did:plc:pat', [
			thread('a', 'negotiating', 'did:plc:neon'),
			thread('b', 'open', 'did:plc:pat', 'harbour-lights'),
			thread('c', 'confirmed', 'did:plc:neon'),
			thread('d', 'declined', 'did:plc:neon')
		])
	});

	for (const [group, show] of [
		['Needs your reply', 'Show a'],
		['In progress', 'Show b'],
		['Confirmed', 'Show c'],
		['Closed', 'Show d']
	]) {
		const section = page.getByRole('region', { name: new RegExp(`^${group}`) });
		await expect.element(section.getByText(show)).toBeInTheDocument();
	}

	const filter = page.getByRole('navigation', { name: 'Filter by project' });
	await expect.element(filter.getByRole('link', { name: 'All' })).toHaveAttribute('aria-current', 'page');
	await expect
		.element(filter.getByRole('link', { name: 'Harbour Lights' }))
		.toHaveAttribute('href', '/dashboard/bookings?project=harbour-lights');
});

it('tells a DJ inquiries come to them, and a promoter where to find DJs', async () => {
	// SAFETY: the empty state checks only that the viewer manages a project.
	const { rerender } = await render(Page, {
		params: {},
		form: null,
		data: data('did:plc:neon', [], [{} as ProjectView])
	});

	await expect.element(page.getByText(/When a promoter sends an inquiry/)).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Find DJs' })).not.toBeInTheDocument();
	await rerender({ data: data('did:plc:pat', []) });
	await expect.element(page.getByRole('link', { name: 'Find DJs' })).toHaveAttribute('href', '/projects');
});
