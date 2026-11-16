import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { BookingEventView, BookingShowLinks, BookingThreadView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';

const viewer = (did: string): Viewer => ({
	did,
	handle: null,
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
});

const SPACE = 'at://did:plc:root/computer.localnetwork.space.booking/3kaaaaaaaaaaa';

const thread = (status: string): BookingThreadView => ({
	space: SPACE,
	skey: '3kaaaaaaaaaaa',
	status,
	title: 'Closing Night',
	showDate: null,
	project: { uri: 'at://did:plc:root/computer.localnetwork.project/2', slug: 'neon-tide', name: 'Neon Tide' },
	promoter: { did: 'did:plc:pat', handle: 'pat.test' },
	participants: [
		{ did: 'did:plc:pat', handle: 'pat.test', role: 'promoter' },
		{ did: 'did:plc:neon', handle: 'neon.test', role: 'manager' }
	],
	lastAuthor: 'did:plc:pat',
	lastActivityAt: '2026-10-01T00:00:00Z',
	createdAt: '2026-10-01T00:00:00Z'
});

const event = (did: string, kind: BookingEventView['kind'], text: string): BookingEventView => ({
	uri: `${SPACE}/${did}/${text}`,
	kind,
	author: { did, handle: null },
	createdAt: '2026-10-01T00:00:00Z',
	text,
	...(kind === 'inquiry' && { show: { title: 'Closing Night', venue: 'Kantine' } })
});

const inquiry = event('did:plc:pat', 'inquiry', 'Free in December?');

const unlinked: BookingShowLinks = { plannerShow: null, announcedShow: null };

const open = (did: string, status = 'open', events = [inquiry], links = unlinked) =>
	render(Page, {
		params: { skey: '3kaaaaaaaaaaa' },
		data: {
			viewer: viewer(did),
			pendingRequests: 0,
			unread: 0,
			waiting: 0,
			needs: [],
			access: Promise.resolve([]),
			sections: Promise.resolve(null),
			projects: Promise.resolve([]),
			thread: thread(status),
			events,
			links
		},
		form: null
	});

const changeStatus = () => page.getByRole('button', { name: 'Change status' });

describe('booking thread status', () => {
	it("gives the project's managers their own moves, and asks before a settling one", async () => {
		await open('did:plc:neon');
		await changeStatus().click();

		for (const name of ['Negotiate', 'Confirm booking', 'Decline'])
			await expect.element(page.getByRole('button', { name, exact: true })).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Withdraw inquiry' })).not.toBeInTheDocument();

		await page.getByRole('button', { name: 'Decline', exact: true }).click();
		await expect.element(page.getByText(/Decline this inquiry\?/)).toBeInTheDocument();
		await expect
			.element(page.getByRole('button', { name: 'Decline', exact: true }))
			.toHaveAttribute('value', 'declined');
		await page.getByRole('button', { name: 'Back' }).click();
		await expect.element(page.getByRole('button', { name: 'Negotiate' })).toBeInTheDocument();
	});

	it('lets the promoter withdraw, and confirm only once a manager has answered', async () => {
		const { rerender } = await open('did:plc:pat');
		await changeStatus().click();

		await expect.element(page.getByRole('button', { name: 'Withdraw inquiry' })).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Confirm booking' })).not.toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Decline' })).not.toBeInTheDocument();

		await rerender({
			data: {
				viewer: viewer('did:plc:pat'),
				pendingRequests: 0,
				unread: 0,
				waiting: 0,
				needs: [],
				access: Promise.resolve([]),
				sections: Promise.resolve(null),
				projects: Promise.resolve([]),
				thread: thread('open'),
				events: [inquiry, event('did:plc:neon', 'message', 'Yes, works for me')],
				links: unlinked
			}
		});
		await expect.element(page.getByRole('button', { name: 'Confirm booking' })).toBeInTheDocument();
	});

	it('offers nothing once a booking is settled, and keeps the thread address out of sight', async () => {
		await open('did:plc:neon', 'declined');

		await expect.element(page.getByText('Declined', { exact: true })).toBeInTheDocument();
		await expect.element(changeStatus()).not.toBeInTheDocument();
		await expect.element(page.getByText(/at:\/\//)).not.toBeInTheDocument();
	});

	it('lets a manager announce a confirmed booking or call it off', async () => {
		await open('did:plc:neon', 'confirmed');

		await expect
			.element(page.getByRole('link', { name: 'Announce this show' }))
			.toHaveAttribute('href', '/dashboard/projects/neon-tide/shows/new?booking=3kaaaaaaaaaaa');
		await changeStatus().click();
		await expect.element(page.getByRole('button', { name: 'Cancel booking' })).toBeInTheDocument();
	});
});

describe('booking thread shows', () => {
	const linked: BookingShowLinks = { plannerShow: '3kplanned', announcedShow: { did: 'did:plc:neon', rkey: '3kshow' } };

	it('links the promoter to the planner show it came from and the show announced from it', async () => {
		await open('did:plc:pat', 'confirmed', [inquiry], linked);

		await expect
			.element(page.getByRole('link', { name: 'In your planner' }))
			.toHaveAttribute('href', '/dashboard/planner/shows/3kplanned');
		await expect
			.element(page.getByRole('link', { name: 'Announced show' }))
			.toHaveAttribute('href', '/projects/neon-tide#shows');
	});

	it('sends whoever announced the show to edit it, and offers no second announcement', async () => {
		await open('did:plc:neon', 'confirmed', [inquiry], { ...linked, plannerShow: null });

		await expect
			.element(page.getByRole('link', { name: 'Announced show' }))
			.toHaveAttribute('href', '/dashboard/projects/neon-tide/shows/3kshow');
		await expect.element(page.getByRole('link', { name: 'In your planner' })).not.toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: 'Announce this show' })).not.toBeInTheDocument();
	});
});
