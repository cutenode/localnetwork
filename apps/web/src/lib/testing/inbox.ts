import type { InboxSubjectView } from '#lib/types.ts';

const hoursAgo = (hours: number) => new Date(Date.now() - hours * 3_600_000).toISOString();

/** A made-up inbox row, about a booking with Neon Tide two hours ago, unread. */
export const inboxRow = (id: string, overrides: Partial<InboxSubjectView> = {}): InboxSubjectView => ({
	id,
	topic: 'bookings',
	title: 'Rooftop Rave',
	scope: 'Neon Tide',
	summary: 'The promoter sent 2 messages and changed the status to Negotiating',
	href: '/dashboard/bookings/3mabc',
	occurredAt: hoursAgo(2),
	unread: true,
	...overrides
});

/** An inbox with unread rows over read ones, the way it usually looks. */
export const inboxRows = [
	inboxRow('a'),
	inboxRow('b', {
		title: 'Halloween at the Depot, a long title that runs past the edge of a phone',
		scope: 'Saffron Social',
		summary: 'The promoter sent a booking inquiry',
		occurredAt: hoursAgo(5)
	}),
	inboxRow('c', {
		title: 'Closing Night',
		summary: 'Neon Tide changed the status to Confirmed',
		occurredAt: hoursAgo(30),
		unread: false
	}),
	inboxRow('d', {
		topic: null,
		title: 'Something changed',
		scope: 'Local Network',
		summary: 'Something changed',
		occurredAt: hoursAgo(200),
		unread: false
	})
];
