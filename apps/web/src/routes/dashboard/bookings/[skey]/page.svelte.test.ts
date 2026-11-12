import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { BookingEventView, BookingThreadView, Viewer } from '#lib/types.ts';
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
	project: {
		uri: 'at://did:plc:root/computer.localnetwork.project/2',
		slug: 'neon-tide',
		name: 'Neon Tide',
		href: '/projects/neon-tide'
	},
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

const open = (did: string, status = 'open', events = [inquiry]) =>
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
			events
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
				events: [inquiry, event('did:plc:neon', 'message', 'Yes, works for me')]
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
