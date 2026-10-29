import { z } from 'zod';
import { BOOKING_STATUSES, labelFor } from '#lib/taxonomy/options.ts';
import type { NotificationTopic } from '#lib/types.ts';
import { parseJson } from '../db';

/** An inbox item in words. */
type Rendered = { title: string; body?: string };

/**
 * A kind of notification: its topic, the data it keeps, and how that reads. Items are put into words when they're
 * read rather than when they're made, so a change of copy reaches the items already there.
 */
function kind<S extends z.ZodType>(definition: {
	topic: NotificationTopic;
	data: S;
	render: (data: z.output<S>) => Rendered;
}) {
	return {
		...definition,
		/** The item in words from its stored JSON, or null when that no longer fits the kind (a later version changed it). */
		view(json: string): Rendered | null {
			const parsed = definition.data.safeParse(parseJson(json, null));

			return parsed.success ? definition.render(parsed.data) : null;
		}
	};
}

/**
 * Something in a booking thread. It names the act, never the manager who wrote, since a manager may be a hidden member
 * (docs/adr/0004), and it holds none of what was written, which stays in the thread's private space.
 */
const booking = z.object({
	/** The thread's key, which its page is at. */
	skey: z.string(),
	/** The show's title, from the inquiry. */
	title: z.string().nullable(),
	projectName: z.string().nullable(),
	/** Which side of the thread did it. */
	from: z.enum(['promoter', 'act'])
});

const who = ({ from, projectName }: z.output<typeof booking>) =>
	from === 'act' ? (projectName ?? 'The DJ') : 'The promoter';

const show = ({ title }: z.output<typeof booking>) => title ?? 'Booking inquiry';

export const KINDS = {
	'booking.inquiry': kind({
		topic: 'bookings',
		data: booking,
		render: (data) => ({
			title: data.projectName ? `New booking inquiry for ${data.projectName}` : 'New booking inquiry',
			body: show(data)
		})
	}),
	'booking.message': kind({
		topic: 'bookings',
		data: booking,
		render: (data) => ({ title: `${who(data)} sent a message`, body: show(data) })
	}),
	'booking.status': kind({
		topic: 'bookings',
		data: booking.extend({ status: z.string() }),
		render: (data) => ({
			title: `${who(data)} marked the booking ${labelFor(BOOKING_STATUSES, data.status).toLowerCase()}`,
			body: show(data)
		})
	})
};

export type NotificationKind = keyof typeof KINDS;

/** The data a kind's producer passes, before its schema fills anything in. */
export type NotificationData<K extends NotificationKind> = z.input<(typeof KINDS)[K]['data']>;

export const isNotificationKind = (value: string): value is NotificationKind => Object.hasOwn(KINDS, value);
