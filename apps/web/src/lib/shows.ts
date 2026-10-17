import type { ShowDetails, ShowRecord } from './types';

/** The lexicon-community calendar event statuses this app writes. */
export const EVENT_STATUS = {
	scheduled: 'community.lexicon.calendar.event#scheduled',
	rescheduled: 'community.lexicon.calendar.event#rescheduled',
	cancelled: 'community.lexicon.calendar.event#cancelled'
} as const;

/** The announce form's fields, as text. */
export type ShowFormValues = {
	name: string;
	date: string;
	start: string;
	end: string;
	venue: string;
	city: string;
	region: string;
	country: string;
	latE6: string;
	lngE6: string;
	tickets: string;
	cancelled: boolean;
};

export const showFormDefaults: ShowFormValues = {
	name: '',
	date: '',
	start: '22:00',
	end: '',
	venue: '',
	city: '',
	region: '',
	country: '',
	latE6: '',
	lngE6: '',
	tickets: '',
	cancelled: false
};

/** The venue's day of a show time, YYYY-MM-DD: show times are written with the venue's offset. */
export const venueDay = (time: string) => time.slice(0, 10);

/** The venue's clock time of a show time, HH:MM. */
export const venueClock = (time: string) => time.slice(11, 16);

/** A show's hours on the venue's clock, "23:00 to 06:00". Lists show its day beside them. */
export function showHours(show: Pick<ShowRecord, 'startsAt' | 'endsAt'>): string {
	const start = venueClock(show.startsAt);

	return show.endsAt ? `${start} to ${venueClock(show.endsAt)}` : start;
}

export const isCancelled = (show: Pick<ShowRecord, 'status'>) => show.status === EVENT_STATUS.cancelled;

/** The announce form filled in from a booking's show details, whose date is the venue's clock stored as UTC. */
export function bookingValues(show: ShowDetails): ShowFormValues {
	const place = show.location;

	return {
		...showFormDefaults,
		name: show.title,
		date: show.date ? venueDay(show.date) : '',
		start: show.date ? venueClock(show.date) : showFormDefaults.start,
		venue: show.venue ?? '',
		city: place?.city ?? '',
		region: place?.region ?? '',
		country: place?.country ?? '',
		latE6: place?.latE6?.toString() ?? '',
		lngE6: place?.lngE6?.toString() ?? ''
	};
}
