import { BOOKING_STATUSES, isOneOf } from './taxonomy/options';
import type { BookingThreadView } from './types';

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
	{ value: 'reply', label: 'Needs your reply' },
	{ value: 'active', label: 'In progress' },
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
