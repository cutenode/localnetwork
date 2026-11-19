import { z } from 'zod';
import { BOOKING_STATUSES, labelFor } from '#lib/taxonomy/options.ts';
import { booking, show, who } from '../booking';
import { kind } from '../kind';

export default kind({
	topic: 'bookings',
	data: booking.extend({ status: z.string() }),
	render: (data) => ({
		title: `${who(data)} marked the booking ${labelFor(BOOKING_STATUSES, data.status).toLowerCase()}`,
		body: show(data)
	})
});
