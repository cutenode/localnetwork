import tzlookup from '@photostructure/tz-lookup';
import type { Location } from '#lib/types.ts';
import { showDate } from './forms/show';

/**
 * A show's times are entered on the venue's clock. Calendar apps read them as instants, so they're written with the
 * venue's UTC offset, found from its coordinates, which also keeps the venue's clock readable in the string.
 */
type Place = Required<Pick<Location, 'latE6' | 'lngE6'>>;

const MINUTE = 60_000;

const pad = (value: number) => String(value).padStart(2, '0');

/** The IANA time zone at a place, like Europe/Berlin, from its coordinates. */
export function zoneOf(place: Place): string {
	return tzlookup(place.latE6 / 1e6, place.lngE6 / 1e6);
}

/** Whether `zone` is an IANA time zone this runtime knows. */
export function isTimeZone(zone: string): boolean {
	try {
		new Intl.DateTimeFormat('en', { timeZone: zone });

		return true;
	} catch {
		return false;
	}
}

/** How many minutes `zone` is ahead of UTC at `instant`. */
function offsetMinutes(zone: string, instant: number): number {
	const name = new Intl.DateTimeFormat('en', { timeZone: zone, timeZoneName: 'longOffset' })
		.formatToParts(instant)
		.find((part) => part.type === 'timeZoneName')?.value;

	const match = /GMT([+-])(\d{2}):(\d{2})/.exec(name ?? '');

	return match ? (match[1] === '-' ? -1 : 1) * (Number(match[2]) * 60 + Number(match[3])) : 0;
}

/** `instant` as an atproto datetime on a clock `offset` minutes ahead of UTC. */
function withOffset(instant: number, offset: number): string {
	const clock = new Date(instant + offset * MINUTE).toISOString().slice(0, 19);

	if (offset === 0) return `${clock}Z`;

	return `${clock}${offset < 0 ? '-' : '+'}${pad(Math.floor(Math.abs(offset) / 60))}:${pad(Math.abs(offset) % 60)}`;
}

/** `date` (YYYY-MM-DD) at `time` (HH:MM) on the clock of `zone` (the venue's), with its offset. */
export function venueTime(date: string, time: string, zone: string): string {
	const clock = Date.parse(showDate(date, time));
	// The offset depends on the instant and the instant on the offset; a second look settles it around a clock change.
	const offset = offsetMinutes(zone, clock - offsetMinutes(zone, clock) * MINUTE);

	return withOffset(clock - offset * MINUTE, offset);
}

const DAY_MS = 86_400_000;

/** The day after `date` (YYYY-MM-DD). */
export const nextDay = (date: string) => new Date(Date.parse(`${date}T00:00:00Z`) + DAY_MS).toISOString().slice(0, 10);

/** `end` (HH:MM) on the venue's clock for a night that starts at `start` on `date`: an end at or before it is the next day. */
export function venueEnd(date: string, start: string, end: string, zone: string): string {
	return venueTime(end > start ? date : nextDay(date), end, zone);
}

/** Any datetime as the clock of `zone` (the venue's) reads it, in the form venueTime writes. */
export function atVenue(value: string, zone: string): string {
	const instant = Date.parse(value);

	return withOffset(instant, offsetMinutes(zone, instant));
}
