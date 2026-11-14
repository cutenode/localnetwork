import { z } from 'zod';
import { pluralize } from '#lib/format.ts';
import { BOOKING_STATUSES, labelFor } from '#lib/taxonomy/options.ts';
import type { NotificationTopic } from '#lib/types.ts';
import { parseJson } from '../db';

/** An inbox item in words. */
type Rendered = { title: string; body?: string };

/**
 * An item's part in its subject's inbox row: what the subject is called, whose initials stand for it, who acted, and
 * what they did, `count` times when there's more than one of this kind from them, this item being the latest.
 */
export type Line = { subject: string; scope: string; who: string; did: (count: number) => string };

/**
 * A kind of notification: its topic, the data it keeps, and how that reads. Items are put into words when they're
 * read rather than when they're made, so a change of copy reaches the items already there.
 */
function kind<S extends z.ZodType>(definition: {
	topic: NotificationTopic;
	data: S;
	render: (data: z.output<S>) => Rendered;
	line: (data: z.output<S>) => Line;
}) {
	/** The item's data from its stored JSON, or null when that no longer fits the kind (a later version changed it). */
	const parse = (json: string) => {
		const parsed = definition.data.safeParse(parseJson(json, null));

		return parsed.success ? parsed.data : null;
	};

	return {
		...definition,
		view(json: string): Rendered | null {
			const data = parse(json);

			return data && definition.render(data);
		},
		lineOf(json: string): Line | null {
			const data = parse(json);

			return data && definition.line(data);
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

/** Who acted, as the middle of a sentence says it. */
const who = ({ from, projectName }: z.output<typeof booking>) =>
	from === 'act' ? (projectName ?? 'the DJ') : 'the promoter';

/** `text` with its first letter capital, to start a sentence. */
export const capitalized = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

const show = ({ title }: z.output<typeof booking>) => title ?? 'Booking inquiry';

const bookingLine = (data: z.output<typeof booking>, did: Line['did']): Line => ({
	subject: show(data),
	scope: data.projectName ?? show(data),
	who: who(data),
	did
});

export const KINDS = {
	'booking.inquiry': kind({
		topic: 'bookings',
		data: booking,
		render: (data) => ({
			title: data.projectName ? `New booking inquiry for ${data.projectName}` : 'New booking inquiry',
			body: show(data)
		}),
		line: (data) =>
			bookingLine(data, (count) => (count === 1 ? 'sent a booking inquiry' : `sent ${count} booking inquiries`))
	}),
	'booking.message': kind({
		topic: 'bookings',
		data: booking,
		render: (data) => ({ title: capitalized(`${who(data)} sent a message`), body: show(data) }),
		line: (data) => bookingLine(data, (count) => `sent ${count === 1 ? 'a message' : pluralize(count, 'message')}`)
	}),
	'booking.status': kind({
		topic: 'bookings',
		data: booking.extend({ status: z.string() }),
		render: (data) => ({
			title: capitalized(`${who(data)} marked the booking ${labelFor(BOOKING_STATUSES, data.status).toLowerCase()}`),
			body: show(data)
		}),
		// Only the latest status counts, however many changes there were.
		line: (data) => bookingLine(data, () => `changed the status to ${labelFor(BOOKING_STATUSES, data.status)}`)
	})
};

export type NotificationKind = keyof typeof KINDS;

/** The topics some kind is about, which are the ones worth offering a choice of. */
export const KIND_TOPICS: NotificationTopic[] = [...new Set(Object.values(KINDS).map((kind) => kind.topic))];

/** The data a kind's producer passes, before its schema fills anything in. */
export type NotificationData<K extends NotificationKind> = z.input<(typeof KINDS)[K]['data']>;

export const isNotificationKind = (value: string): value is NotificationKind => Object.hasOwn(KINDS, value);
