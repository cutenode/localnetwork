import { UserError } from '../core';
import { isWebUrl, limit, optionalInt, optionalText, text } from '../http';
import type { ShowInput, Venue } from '../services/shows';

const CLOCK = /^([01]\d|2[0-3]):[0-5]\d$/;

/** Parses the announce form. Times are on the venue's clock; an end at or before the start is the next day. */
export function parseAnnouncement(form: FormData): ShowInput {
	const name = limit('Show name', text(form, 'name', 1200), 120);

	if (!name) throw new UserError('Give the show a name');
	const date = text(form, 'date', 10);
	const start = text(form, 'start', 5);
	const end = text(form, 'end', 5);

	if (!date || !CLOCK.test(start)) throw new UserError('Pick a date and start time');

	if (end && !CLOCK.test(end)) throw new UserError('Pick a valid end time');
	const city = limit('City', text(form, 'city', 1000), 100);
	const country = text(form, 'country', 100).toUpperCase();
	const latE6 = optionalInt(form, 'latE6', -90_000_000, 90_000_000);
	const lngE6 = optionalInt(form, 'lngE6', -180_000_000, 180_000_000);

	// The coordinates find the venue's clock, and so the time the show starts everywhere else.
	if (!city || !/^[A-Z]{2}$/.test(country) || latE6 === undefined || lngE6 === undefined)
		throw new UserError("Pick the venue's city from the search");
	const location: Venue = { city, country, latE6, lngE6 };
	const region = limit('Region', optionalText(form, 'region', 1000), 100);

	if (region) location.region = region;
	const tickets = optionalText(form, 'tickets', 2000);

	if (tickets && !isWebUrl(tickets)) throw new UserError('Use a full web address for tickets, starting with https://');

	const input: ShowInput = { name, date, start, location, cancelled: form.get('cancelled') === 'on' };
	const venue = limit('Venue', optionalText(form, 'venue', 1200), 120);

	if (end) input.end = end;

	if (venue) input.venue = venue;

	if (tickets) input.tickets = tickets;

	return input;
}
