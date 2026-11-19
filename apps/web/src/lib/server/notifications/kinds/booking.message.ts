import { pluralize } from '#lib/format.ts';
import { booking, bookingLine, show, who } from '../booking';
import { capitalized, kind } from '../kind';

export default kind({
	topic: 'bookings',
	data: booking,
	render: (data) => ({ title: capitalized(`${who(data)} sent a message`), body: show(data) }),
	line: (data) => bookingLine(data, (count) => `sent ${count === 1 ? 'a message' : pluralize(count, 'message')}`)
});
