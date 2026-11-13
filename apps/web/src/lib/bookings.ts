import { BOOKING_STATUSES, isOneOf } from './taxonomy/options';
import type { BookingListThread, BookingThreadView } from './types';

export type BookingStatus = (typeof BOOKING_STATUSES)[number]['value'];

/** Which side of a booking thread someone is on: the promoter who asked, or one of the project's managers. */
export type BookingRole = 'promoter' | 'manager';

/**
 * A status change one side can make, with its button label. `settles` changes ask first: they end the booking or fix
 * it, and the other side reads them as final. `ask` is that question.
 */
export type BookingAction = { status: BookingStatus; label: string; settles?: { ask: string } };

const negotiate: BookingAction = { status: 'negotiating', label: 'Negotiate' };

const confirm: BookingAction = {
	status: 'confirmed',
	label: 'Confirm booking',
	settles: { ask: 'Confirm this booking? Both sides will see it as booked.' }
};

const decline: BookingAction = {
	status: 'declined',
	label: 'Decline',
	settles: { ask: "Decline this inquiry? The thread closes and can't be reopened." }
};

const withdraw: BookingAction = {
	status: 'cancelled',
	label: 'Withdraw inquiry',
	settles: { ask: "Withdraw this inquiry? The thread closes and can't be reopened." }
};

const cancel: BookingAction = {
	status: 'cancelled',
	label: 'Cancel booking',
	settles: { ask: "Cancel this booking? The thread closes and can't be reopened." }
};

/**
 * What each side can move a thread to from each status. Declined and cancelled threads are settled, so nothing
 * leaves them. Managers answer an inquiry; the promoter can confirm once a manager has answered, or walk away.
 */
const ACTIONS: Record<BookingRole, Partial<Record<BookingStatus, BookingAction[]>>> = {
	manager: { open: [negotiate, confirm, decline], negotiating: [confirm, decline], confirmed: [cancel] },
	promoter: { open: [confirm, withdraw], negotiating: [confirm, withdraw], confirmed: [cancel] }
};

/**
 * The status changes `role` can make to a thread at `status`. A promoter can't confirm an open inquiry until a
 * manager has written in the thread (`answered`), so nobody confirms a booking the other side hasn't seen.
 */
export function bookingActions(role: BookingRole, status: string, answered: boolean): BookingAction[] {
	const actions = (isOneOf(BOOKING_STATUSES, status) && ACTIONS[role][status]) || [];

	if (role === 'promoter' && status === 'open' && !answered) return actions.filter((action) => action !== confirm);

	return actions;
}

/** How the bookings list sorts threads for whoever is looking, in the order it shows them. */
export const BOOKING_GROUPS = [
	{ value: 'reply', label: 'Your turn' },
	{ value: 'active', label: 'Waiting on them' },
	{ value: 'confirmed', label: 'Confirmed' },
	{ value: 'closed', label: 'Closed' }
] as const;

export type BookingGroup = (typeof BOOKING_GROUPS)[number]['value'];

/** Where a thread sits for `viewer`: a live one waits on them when someone on the other side wrote in it last. */
export function bookingGroup(
	thread: Pick<BookingThreadView, 'status' | 'lastAuthor'> & { participants: { did: string; role: string }[] },
	viewer: string
): BookingGroup {
	if (thread.status === 'confirmed') return 'confirmed';

	if (thread.status === 'declined' || thread.status === 'cancelled') return 'closed';
	const roleOf = (did: string) => thread.participants.find((participant) => participant.did === did)?.role;

	return thread.lastAuthor && roleOf(thread.lastAuthor) !== roleOf(viewer) ? 'reply' : 'active';
}

/** Whether a thread's night went by, by the UTC day show dates are kept in. */
export const datePassed = ({ showDate }: Pick<BookingThreadView, 'showDate'>, now = Date.now()) =>
	!!showDate && showDate.slice(0, 10) < new Date(now).toISOString().slice(0, 10);

/**
 * The threads by turn, in the order the list shows them. Within a group the nights still to come lead, soonest first,
 * then threads with no date, then ones whose date went by. Closed keeps the newest on top, and takes confirmed
 * bookings once their night has passed, so Confirmed only ever holds what's ahead.
 */
export function bookingsByTurn<Thread extends Parameters<typeof bookingGroup>[0] & Pick<BookingThreadView, 'showDate'>>(
	threads: Thread[],
	viewer: string,
	now = Date.now()
) {
	const groupOf = (thread: Thread): BookingGroup => {
		const group = bookingGroup(thread, viewer);

		return group === 'confirmed' && datePassed(thread, now) ? 'closed' : group;
	};

	const rank = (thread: Thread) => {
		if (!thread.showDate) return 1;

		return datePassed(thread, now) ? 2 : 0;
	};

	const byNight = (a: Thread, b: Thread) => rank(a) - rank(b) || (a.showDate ?? '').localeCompare(b.showDate ?? '');

	return BOOKING_GROUPS.flatMap((group) => {
		const inGroup = threads.filter((thread) => groupOf(thread) === group.value);

		if (inGroup.length === 0) return [];

		return [{ ...group, threads: group.value === 'closed' ? inGroup : inGroup.toSorted(byNight) }];
	});
}

/**
 * The status a row in `group` adds to what the group already says, if any: how a closed thread ended, a live one's
 * date going by, and otherwise how far along it is, where an inquiry the manager has to answer is new. A confirmed row
 * in Closed got there by its date passing.
 */
export function bookingMark(
	thread: Pick<BookingListThread, 'status' | 'showDate' | 'withdrawn'>,
	group: BookingGroup,
	role: BookingRole | undefined,
	now = Date.now()
): string | null {
	if (group === 'confirmed') return null;

	if (group === 'closed') {
		if (thread.withdrawn) return 'withdrawn';

		return thread.status === 'confirmed' ? 'date_passed' : thread.status;
	}

	if (datePassed(thread, now)) return 'date_passed';

	if (thread.status === 'negotiating') return 'negotiating';

	return group === 'reply' && role === 'manager' ? 'new' : null;
}
