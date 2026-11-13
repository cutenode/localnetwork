import { expect, it } from 'vitest';
import { bookingGroup, bookingMark, bookingsByTurn, datePassed } from './bookings';

const NOW = Date.parse('2026-11-06T12:00:00Z');

const participants = [
	{ did: 'pat', role: 'promoter' },
	{ did: 'neon', role: 'manager' }
];

const thread = (title: string, status: string, showDate: string | null = null, lastAuthor = 'pat') => ({
	title,
	status,
	showDate,
	lastAuthor,
	participants
});

const titles = (groups: ReturnType<typeof bookingsByTurn<ReturnType<typeof thread>>>) =>
	Object.fromEntries(groups.map((group) => [group.value, group.threads.map(({ title }) => title)]));

it('counts a date as passed from the day after it', () => {
	expect(datePassed({ showDate: '2026-11-05' }, NOW)).toBe(true);
	expect(datePassed({ showDate: '2026-11-06' }, NOW)).toBe(false);
	expect(datePassed({ showDate: '2026-11-06T23:00:00.000Z' }, NOW)).toBe(false);
	expect(datePassed({ showDate: null }, NOW)).toBe(false);
});

it('orders a group by night: soonest first, then no date, then gone by', () => {
	const threads = [
		thread('Gone', 'open', '2026-08-01'),
		thread('Undated', 'open'),
		thread('Later', 'open', '2026-12-12'),
		thread('Soon', 'open', '2026-11-14')
	];

	expect(titles(bookingsByTurn(threads, 'neon', NOW))).toEqual({ reply: ['Soon', 'Later', 'Undated', 'Gone'] });
});

it('sends a confirmed booking to Closed once its night passed, and keeps Closed newest first', () => {
	const threads = [
		thread('Newest', 'declined', '2026-01-01'),
		thread('Played', 'confirmed', '2026-08-01'),
		thread('Ahead', 'confirmed', '2026-12-12')
	];

	expect(titles(bookingsByTurn(threads, 'neon', NOW))).toEqual({
		confirmed: ['Ahead'],
		closed: ['Newest', 'Played']
	});
});

it("leaves a live thread whose date passed in its turn, as bookingGroup and the sidebar's count have it", () => {
	const late = thread('Late', 'negotiating', '2026-08-01');

	expect(bookingGroup(late, 'neon')).toBe('reply');
	expect(titles(bookingsByTurn([late], 'neon', NOW))).toEqual({ reply: ['Late'] });
});

it('marks only what the group leaves unsaid', () => {
	const mark = (
		status: string,
		group: Parameters<typeof bookingMark>[1],
		showDate: string | null = null,
		withdrawn = false
	) => bookingMark({ status, showDate, withdrawn }, group, 'manager', NOW);

	expect(mark('open', 'reply')).toBe('new');
	expect(bookingMark({ status: 'open', showDate: null, withdrawn: false }, 'reply', 'promoter', NOW)).toBeNull();
	expect(mark('open', 'active')).toBeNull();
	expect(mark('negotiating', 'active')).toBe('negotiating');
	expect(mark('confirmed', 'confirmed')).toBeNull();
	expect(mark('negotiating', 'reply', '2026-08-01')).toBe('date_passed');
	expect(mark('declined', 'closed')).toBe('declined');
	expect(mark('cancelled', 'closed')).toBe('cancelled');
	expect(mark('cancelled', 'closed', null, true)).toBe('withdrawn');
	expect(mark('confirmed', 'closed', '2026-08-01')).toBe('date_passed');
});
