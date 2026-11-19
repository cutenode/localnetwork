import type { JsonObject } from '@atproto/lex-json';

export const ADDRESS = 'community.lexicon.location.address';

export const GEO = 'community.lexicon.location.geo';

/** A calendar event as this app writes it, with the fields a show copies. */
export type CalendarEvent = JsonObject & { startsAt: string; endsAt?: string; status: string };

/** What this app says about a night in its `community.lexicon.calendar.event`, whether it's a show or a party's event. */
export type CalendarNight = {
	name: string;
	description?: string;
	startsAt: string;
	endsAt?: string;
	/** One of the calendar event's status tokens (EVENT_STATUS). */
	status: string;
	/** Where it is: an address and coordinates (ADDRESS, GEO), and any other location the lexicon takes. */
	locations: JsonObject[];
	/** Its links, the ticket link first. */
	uris: { uri: string; name: string }[];
	createdAt: string;
};

/**
 * The `community.lexicon.calendar.event` for `night`, in person, over `base` (the event as it is now, when another
 * calendar app may have added to it) so what it added stays. The end and links are replaced, and gone when `night` has
 * none; a description is only ever added, since a show never writes one.
 */
export function calendarEvent(night: CalendarNight, base: JsonObject | null = null): CalendarEvent {
	const { endsAt, description, uris, ...rest } = night;
	const event: CalendarEvent = { ...base, ...rest, mode: 'community.lexicon.calendar.event#inperson' };

	delete event.endsAt;
	delete event.uris;

	if (endsAt) event.endsAt = endsAt;

	if (uris.length) event.uris = uris;

	if (description) event.description = description;

	return event;
}
