import { z } from 'zod';
import { BOOKING_STATUSES, labelFor } from '#lib/taxonomy/options.ts';
import { booking, bookingLine, show, who } from '../booking';
import { capitalized, kind } from '../kind';

export default kind({
	topic: 'bookings',
	data: booking.extend({ status: z.string() }),
	render: (data) => ({
		title: capitalized(`${who(data)} marked the booking ${labelFor(BOOKING_STATUSES, data.status).toLowerCase()}`),
		body: show(data)
	}),
	// Only the latest status counts, however many changes there were.
	line: (data) => bookingLine(data, () => `changed the status to ${labelFor(BOOKING_STATUSES, data.status)}`)
});
