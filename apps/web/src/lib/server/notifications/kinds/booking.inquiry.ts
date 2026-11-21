import { booking, bookingLine, show } from '../booking';
import { kind } from '../kind';

export default kind({
	topic: 'bookings',
	data: booking,
	render: (data) => ({
		title: data.projectName ? `New booking inquiry for ${data.projectName}` : 'New booking inquiry',
		body: show(data)
	}),
	line: (data) =>
		bookingLine(data, (count) => (count === 1 ? 'sent a booking inquiry' : `sent ${count} booking inquiries`))
});
