import { booking, show, who } from '../booking';
import { kind } from '../kind';

export default kind({
	topic: 'bookings',
	data: booking,
	render: (data) => ({ title: `${who(data)} sent a message`, body: show(data) })
});
