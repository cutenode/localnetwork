import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { BookingEventView, BookingShowLinks, BookingThreadView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';
import type { PageProps } from './$types';

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

const unlinked: BookingShowLinks = { plannerShow: null, announcedShow: null };

const data = (
	did: string,
	status = 'open',
	events = [inquiry],
	overrides: Partial<PageProps['data']> = {}
): PageProps['data'] => ({
	viewer: viewer(did),
	pendingRequests: 0,
	unread: 0,
	waiting: 0,
	needs: [],
	access: Promise.resolve([]),
	sections: Promise.resolve(null),
	projects: Promise.resolve([]),
	thread: { ...thread(status), project: null },
	project: thread(status).project,
	events,
	links: unlinked,
	...overrides
});

const open = (did: string, status = 'open', events = [inquiry], overrides: Partial<PageProps['data']> = {}) =>
	render(Page, { params: { skey: '3kaaaaaaaaaaa' }, data: data(did, status, events, overrides), form: null });

describe('booking thread status', () => {
	it("gives the project's managers their own moves, and asks before a settling one", async () => {
		await open('did:plc:neon');

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

		await expect.element(page.getByRole('button', { name: 'Withdraw inquiry' })).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Confirm booking' })).not.toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Decline' })).not.toBeInTheDocument();

		await rerender({
			data: data('did:plc:pat', 'open', [inquiry, event('did:plc:neon', 'message', 'Yes, works for me')])
		});
		await expect.element(page.getByRole('button', { name: 'Confirm booking' })).toBeInTheDocument();
	});

	it('offers nothing once a booking is settled, and keeps the thread address out of sight', async () => {
		await open('did:plc:neon', 'declined');

		await expect.element(page.getByText('Declined', { exact: true })).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Confirm booking' })).not.toBeInTheDocument();
		await expect.element(page.getByText(/at:\/\//)).not.toBeInTheDocument();
	});

	it('lets a manager announce a confirmed booking or call it off', async () => {
		await open('did:plc:neon', 'confirmed');

		await expect
			.element(page.getByRole('link', { name: 'Announce this show' }))
			.toHaveAttribute('href', '/dashboard/projects/neon-tide/shows/new?booking=3kaaaaaaaaaaa');
		await expect.element(page.getByRole('button', { name: 'Cancel booking' })).toBeInTheDocument();
	});

	it('takes the move back to the buttons after Back, and keeps the confirm step for one that settles', async () => {
		await open('did:plc:pat', 'negotiating');

		await page.getByRole('button', { name: 'Withdraw inquiry' }).click();
		await expect.element(page.getByText(/Withdraw this inquiry\?/)).toHaveFocus();
		await expect.element(page.getByLabelText('Note')).toBeInTheDocument();
		await page.getByRole('button', { name: 'Back' }).click();
		await expect.element(page.getByRole('heading', { name: 'Status' })).toHaveFocus();
		await expect.element(page.getByText(/Withdraw this inquiry\?/)).not.toBeInTheDocument();
	});

	it('names a cancelled inquiry nobody confirmed as withdrawn', async () => {
		await open('did:plc:neon', 'cancelled', [inquiry, { ...event('did:plc:pat', 'status', ''), status: 'cancelled' }]);

		await expect.element(page.getByText('Withdrawn', { exact: true })).toBeInTheDocument();
	});

	it('keeps Cancelled for a booking that was confirmed', async () => {
		await open('did:plc:neon', 'cancelled', [
			inquiry,
			{ ...event('did:plc:neon', 'status', ''), status: 'confirmed' },
			{ ...event('did:plc:pat', 'status', 'x'), status: 'cancelled' }
		]);

		await expect.element(page.getByText('Cancelled', { exact: true })).toBeInTheDocument();
	});

	it('marks a date that went by', async () => {
		const past = { ...thread('negotiating'), project: null, showDate: '2020-08-01' };

		await open('did:plc:neon', 'negotiating', [inquiry], { thread: past });

		await expect.element(page.getByText('Date passed')).toBeInTheDocument();
	});
});

describe('whose turn and who wrote', () => {
	it('says whose turn it is', async () => {
		await open('did:plc:neon');
		await expect.element(page.getByText('Your turn')).toBeInTheDocument();
	});

	it('tells a promoter they wait on the act by name, and shows the act speaking as itself', async () => {
		const reply = {
			...event('did:plc:neon', 'message', 'Works for me'),
			author: { did: 'did:plc:neon', handle: 'neon.test' }
		};

		await open('did:plc:pat', 'negotiating', [inquiry, reply], {
			thread: { ...thread('negotiating'), project: null, lastAuthor: 'did:plc:pat' }
		});

		await expect.element(page.getByText(/Waiting on\s+Neon Tide/)).toBeInTheDocument();
		await expect.element(page.getByText('Works for me')).toBeInTheDocument();
		await expect.element(page.getByText(/neon\.test/)).not.toBeInTheDocument();
	});

	it("doesn't wait on the directory for the page, and can do without it", async () => {
		const { rerender } = await open('did:plc:pat', 'open', [inquiry], { project: new Promise<never>(() => {}) });

		await expect.element(page.getByRole('heading', { level: 1, name: 'Closing Night' })).toBeVisible();
		await expect.element(page.getByText('Free in December?')).toBeVisible();
		await rerender({ data: data('did:plc:pat', 'open', [inquiry], { project: null }) });
		await expect.element(page.getByText(/You and\s+the act/)).toBeInTheDocument();
	});

	it('holds the announce button until the act is known', async () => {
		await open('did:plc:neon', 'confirmed', [inquiry], { project: null });

		await expect.element(page.getByRole('link', { name: 'Announce this show' })).not.toBeInTheDocument();
	});
});

describe('booking thread shows', () => {
	const linked: BookingShowLinks = { plannerShow: '3kplanned', announcedShow: { did: 'did:plc:neon', rkey: '3kshow' } };

	it('links the promoter to the planner show it came from and the show announced from it', async () => {
		await open('did:plc:pat', 'confirmed', [inquiry], { links: linked });

		await expect
			.element(page.getByRole('link', { name: 'In your planner' }))
			.toHaveAttribute('href', '/dashboard/planner/shows/3kplanned');
		await expect
			.element(page.getByRole('link', { name: 'Announced show' }))
			.toHaveAttribute('href', '/projects/neon-tide#shows');
	});

	it('sends whoever announced the show to edit it, and offers no second announcement', async () => {
		await open('did:plc:neon', 'confirmed', [inquiry], { links: { ...linked, plannerShow: null } });

		await expect
			.element(page.getByRole('link', { name: 'Announced show' }))
			.toHaveAttribute('href', '/dashboard/projects/neon-tide/shows/3kshow');
		await expect.element(page.getByRole('link', { name: 'In your planner' })).not.toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: 'Announce this show' })).not.toBeInTheDocument();
	});
});

describe('a thread with a listed DJ', () => {
	const MARA = 'did:plc:mara';

	const mara = {
		uri: `at://${MARA}/computer.localnetwork.profile/self`,
		slug: null,
		name: 'Mara',
		href: '/@mara.test'
	};

	const withDj = (status: string, lastAuthor = 'did:plc:pat') => ({
		thread: {
			...thread(status),
			project: null,
			lastAuthor,
			participants: [
				{ did: 'did:plc:pat', handle: 'pat.test', role: 'promoter' as const },
				{ did: MARA, handle: 'mara.test', role: 'performer' as const }
			]
		},
		project: mara
	});

	it('gives the DJ the moves and turn an act manager gets, with no show to announce', async () => {
		const { rerender } = await open(MARA, 'open', [inquiry], withDj('open'));

		await expect.element(page.getByText('Your turn')).toBeInTheDocument();

		for (const name of ['Negotiate', 'Confirm booking', 'Decline'])
			await expect.element(page.getByRole('button', { name, exact: true })).toBeInTheDocument();

		await rerender({ data: data(MARA, 'confirmed', [inquiry], withDj('confirmed', MARA)) });
		await expect.element(page.getByRole('button', { name: 'Cancel booking' })).toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: 'Announce this show' })).not.toBeInTheDocument();
	});

	it("links the promoter to the DJ's page, and lets them confirm once the DJ has answered", async () => {
		const answer = { ...event(MARA, 'message', 'Yes'), author: { did: MARA, handle: 'mara.test' } };

		await open('did:plc:pat', 'open', [inquiry, answer], withDj('open', MARA));

		await expect.element(page.getByRole('link', { name: 'Mara', exact: true })).toHaveAttribute('href', '/@mara.test');
		await expect.element(page.getByRole('button', { name: 'Confirm booking' })).toBeInTheDocument();
		await expect.element(page.getByText(/mara\.test/)).not.toBeInTheDocument();
	});
});
