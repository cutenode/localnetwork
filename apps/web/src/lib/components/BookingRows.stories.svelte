<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { type BookingGroup, bookingsByTurn } from '#lib/bookings.page-only.ts';
	import { light } from '#lib/testing/light.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import type { BookingListThread } from '#lib/types.ts';
	import BookingRows from './BookingRows.svelte';

	/** One group of the bookings list, for the act's manager and for the promoter, with the threads Patterns/Bookings shows. */
	const { Story } = defineMeta({ title: 'Dashboard/Booking rows', component: BookingRows, tags: ['autodocs'] });

	const NEON = 'did:plc:neon';

	const PAT = 'did:plc:pat';

	const day = (offset: number) => new Date(Date.now() + offset * 86_400_000).toISOString().slice(0, 10);

	const hoursAgo = (hours: number) => new Date(Date.now() - hours * 3_600_000).toISOString();

	const neonTide = { uri: 'at://did:plc:root/computer.localnetwork.project/neon-tide', slug: 'neon-tide', name: 'Neon Tide', href: '/projects/neon-tide' };

	const thread = (skey: string, title: string | null, status: string, showDate: string | null, lastAuthor: string, overrides: Partial<BookingListThread> = {}): BookingListThread => ({
		space: `at:space/did:plc:root/computer.localnetwork.space.booking/${skey}`,
		skey,
		status,
		title,
		showDate,
		project: neonTide,
		promoter: { did: PAT, handle: 'pat.test' },
		participants: [
			{ did: PAT, handle: 'pat.test', role: 'promoter' },
			{ did: NEON, handle: 'neon.test', role: 'manager' }
		],
		lastAuthor,
		lastActivityAt: hoursAgo(Number(skey) + 1),
		createdAt: hoursAgo(72),
		withdrawn: false,
		...overrides
	});

	const threads = [
		thread('1', 'Rooftop Rave', 'open', day(8), PAT),
		thread('2', 'Basement Sessions', 'negotiating', day(21), PAT),
		thread('3', 'Walkthrough Night', 'open', null, PAT),
		thread('4', 'Summer Terrace', 'negotiating', day(-97), PAT),
		thread('5', 'Closing Night', 'open', day(15), NEON),
		thread('6', 'Late Lounge', 'negotiating', day(51), NEON),
		thread('7', 'Warehouse Weekender', 'confirmed', day(71), NEON),
		thread('8', 'Garden Party', 'declined', day(29), NEON),
		thread('9', 'Pop Up Night', 'cancelled', day(43), PAT, { withdrawn: true }),
		thread('10', 'Harbour Boat Party', 'cancelled', day(92), NEON),
		thread('11', 'Spring Opening', 'confirmed', day(-209), NEON)
	];

	/** The threads the bookings page puts in `group` for `viewer`, in its order. */
	const rows = (viewer: string, group: BookingGroup, list = threads) => bookingsByTurn(list, viewer).find(({ value }) => value === group)?.threads ?? [];

	const unnamed = (list: BookingListThread[]) => list.map((item) => ({ ...item, project: null }));

	type Play = { canvasElement: HTMLElement };

	const titles = (canvasElement: HTMLElement) => within(canvasElement).getAllByRole('link').map((link) => link.textContent);

	// The act's manager answers inquiries, so the ones still open are new to them.
	const yourTurnForTheAct = async ({ canvasElement }: Play) => {
		const canvas = within(canvasElement);

		await expect(titles(canvasElement)).toEqual(['Rooftop Rave', 'Basement Sessions', 'Walkthrough Night', 'Summer Terrace']);
		await expect(canvas.getByRole('link', { name: 'Rooftop Rave' })).toHaveAttribute('href', '/dashboard/bookings/1');
		await expect(canvas.getAllByText('New')).toHaveLength(2);
		await expect(canvas.getByText('Negotiating')).toBeVisible();
		await expect(canvas.getByText('Date passed')).toBeVisible();
		await expect(canvas.getByText('No date')).toBeVisible();
		await expect(canvas.getAllByText(/^Neon Tide · from @pat\.test · /)).toHaveLength(4);
	};

	const waitingForTheAct = async ({ canvasElement }: Play) => {
		const canvas = within(canvasElement);

		await expect(titles(canvasElement)).toEqual(['Closing Night', 'Late Lounge']);
		await expect(canvas.getAllByText(/^Waiting on @pat\.test · Neon Tide · you wrote /)).toHaveLength(2);
		await expect(canvas.getByText('Negotiating')).toBeVisible();
	};

	// Nothing is new to the promoter: they sent it.
	const yourTurnForThePromoter = async ({ canvasElement }: Play) => {
		const canvas = within(canvasElement);

		await expect(titles(canvasElement)).toEqual(['Closing Night', 'Late Lounge']);
		await expect(canvas.queryByText('New')).toBeNull();
		await expect(canvas.getAllByText(/^Neon Tide · you asked · /)).toHaveLength(2);
	};

	const waitingForThePromoter = async ({ canvasElement }: Play) => {
		const canvas = within(canvasElement);

		await expect(canvas.getAllByText(/^Waiting on Neon Tide · you wrote /)).toHaveLength(4);
		await expect(canvas.getByText('No date')).toBeVisible();
	};

	// The group says it's confirmed, so the row doesn't.
	const confirmed = async ({ canvasElement }: Play) => {
		const canvas = within(canvasElement);

		await expect(titles(canvasElement)).toEqual(['Warehouse Weekender']);
		await expect(canvas.queryByText('Confirmed')).toBeNull();
		await expect(canvas.getByText(/^Neon Tide · from @pat\.test · /)).toBeVisible();
	};

	const settled = async ({ canvasElement }: Play) => {
		const canvas = within(canvasElement);

		for (const pill of ['Declined', 'Withdrawn', 'Cancelled', 'Date passed']) await expect(canvas.getByText(pill)).toBeVisible();
		await expect(canvas.getAllByText(/ · from @pat\.test · /)).toHaveLength(4);
	};

	const settledForThePromoter = async ({ canvasElement }: Play) => {
		await expect(within(canvasElement).getAllByText(/^Neon Tide · you asked · /)).toHaveLength(4);
	};

	// The act's name holds its room until the directory answers, and the rest of the line is there already.
	const actsPending = async ({ canvasElement }: Play) => {
		const canvas = within(canvasElement);

		await expect(canvas.queryByText(/Neon Tide/)).toBeNull();
		await expect(canvas.getAllByText(/^Waiting on @pat\.test · +· you wrote /)).toHaveLength(2);
	};

	const directoryDown = async ({ canvasElement }: Play) => {
		await expect(within(canvasElement).getAllByText(/^Waiting on the act · you wrote /)).toHaveLength(4);
	};

	const untitled = async ({ canvasElement }: Play) => {
		const canvas = within(canvasElement);

		await expect(canvas.getByRole('link', { name: 'Booking inquiry' })).toHaveAttribute('href', '/dashboard/bookings/12');
		await expect(canvas.getByText('No date')).toBeVisible();
	};

	const loading = async ({ canvasElement }: Play) => {
		const canvas = within(canvasElement);

		await expect(canvas.getByRole('list')).toHaveAttribute('aria-busy', 'true');
		await expect(canvas.getAllByRole('listitem')).toHaveLength(3);
		await expect(canvas.queryAllByRole('link')).toHaveLength(0);
	};

	const empty = async ({ canvasElement }: Play) => {
		const canvas = within(canvasElement);

		await expect(canvas.getByRole('list')).not.toHaveAttribute('aria-busy');
		await expect(canvas.queryAllByRole('listitem')).toHaveLength(0);
	};
</script>

<Story name="Your turn, for the act" args={{ threads: rows(NEON, 'reply'), group: 'reply', viewer: NEON }} play={yourTurnForTheAct} />

<Story name="Your turn, for the act, light" args={{ threads: rows(NEON, 'reply'), group: 'reply', viewer: NEON }} play={yourTurnForTheAct} beforeEach={light} />

<Story name="Your turn, for the act, on a phone" args={{ threads: rows(NEON, 'reply'), group: 'reply', viewer: NEON }} play={yourTurnForTheAct} globals={phone} />

<Story name="Waiting on the promoter" args={{ threads: rows(NEON, 'active'), group: 'active', viewer: NEON }} play={waitingForTheAct} />

<Story name="Your turn, for the promoter" args={{ threads: rows(PAT, 'reply'), group: 'reply', viewer: PAT }} play={yourTurnForThePromoter} />

<Story name="Waiting on the act" args={{ threads: rows(PAT, 'active'), group: 'active', viewer: PAT }} play={waitingForThePromoter} />

<Story name="Waiting on the act, on a phone" args={{ threads: rows(PAT, 'active'), group: 'active', viewer: PAT }} play={waitingForThePromoter} globals={phone} />

<Story name="Confirmed" args={{ threads: rows(NEON, 'confirmed'), group: 'confirmed', viewer: NEON }} play={confirmed} />

<Story name="Settled" args={{ threads: rows(NEON, 'settled'), group: 'settled', viewer: NEON }} play={settled} />

<Story name="Settled, for the promoter" args={{ threads: rows(PAT, 'settled'), group: 'settled', viewer: PAT }} play={settledForThePromoter} />

<Story name="Loading act names" args={{ threads: unnamed(rows(NEON, 'active')), group: 'active', viewer: NEON, actsPending: true }} play={actsPending} />

<Story name="Act names didn't load" args={{ threads: unnamed(rows(PAT, 'active')), group: 'active', viewer: PAT }} play={directoryDown} />

<Story name="No title or date" args={{ threads: [thread('12', null, 'open', null, PAT)], group: 'reply', viewer: NEON }} play={untitled} />

<Story name="Loading" args={{ loading: 3 }} play={loading} />

<Story name="Empty" args={{ threads: [] }} play={empty} />
