import type { EditChoice, FieldValues } from '#lib/components/edit/types.ts';
import { venueClock, venueDay } from '#lib/shows.ts';
import type { EventRecord } from '#lib/types.ts';
import { FieldError, UserError } from '../core';
import { optionalText, text } from '../http';
import type { EventInput, ZoneChangeError } from '../services/events';
import { blockSubject } from '../venue-blocks';

const ZONE_CHANGES = ['clock', 'instant'] as const;

/**
 * An event page's form as what saving it writes. Each lineup row names its performer by a handle or an act's page link,
 * with set times; a row left empty is skipped. Publish is the button that sent it.
 */
export async function parseEventForm(app: Parameters<typeof blockSubject>[0], form: FormData): Promise<EventInput> {
	const name = text(form, 'name', 1200);

	if (!name) throw new FieldError('Give the event a name', 'name');
	const [starts, ends] = [form.getAll('setStart'), form.getAll('setEnd')];
	const lineup: NonNullable<EventInput['lineup']> = [];

	for (const [row, entry] of form.getAll('performer').entries()) {
		const named = entry instanceof File ? '' : entry.trim();

		if (!named) continue;
		// An act goes by its page's link; anyone else by their handle.
		const kind = /(?:^|\/)act\//.test(named) ? 'act' : 'profile';

		const performer = await blockSubject(app, kind, named).catch((cause: unknown) => {
			throw cause instanceof UserError ? new FieldError(cause.message, 'performer', row) : cause;
		});

		const [start, end] = [starts[row], ends[row]].map((time) => (time instanceof File ? undefined : time || undefined));
		lineup.push({ performer, start, end });
	}

	return {
		name,
		description: optionalText(form, 'description', 30_000),
		venue: optionalText(form, 'venue', 500),
		date: optionalText(form, 'date', 10),
		start: optionalText(form, 'start', 5),
		end: optionalText(form, 'end', 5),
		tickets: optionalText(form, 'tickets', 2000),
		timeZone: optionalText(form, 'timeZone', 64),
		zoneChange: ZONE_CHANGES.find((choice) => choice === form.get('zoneChange')),
		audience: form.get('audience') === 'private' ? 'private' : 'public',
		lineup,
		publish: form.get('publish') === 'on'
	};
}

/**
 * An event's saved page as its form's text values, for a save that met a newer version. The venue and the lineup are
 * pickers and rows rather than text, so they're left to what this editor has.
 */
export const eventValues = (event: EventRecord): FieldValues => ({
	name: event.name,
	description: event.description ?? '',
	date: event.startsAt ? venueDay(event.startsAt) : '',
	start: event.startsAt ? venueClock(event.startsAt) : '',
	end: event.endsAt ? venueClock(event.endsAt) : '',
	timeZone: event.timeZone ?? '',
	tickets: event.tickets ?? '',
	audience: event.audience === 'public' ? 'public' : 'private'
});

/** The last part of a time zone's name, the way people say it: Europe/Berlin is Berlin, America/New_York New York. */
const zoneCity = (zone: string) => zone.split('/').at(-1)!.replaceAll('_', ' ');

/**
 * What the editor is asked when a save would move the times onto another clock (ZoneChangeError): keep the times as
 * entered on the new clock, or the same moments, each with the start it would save there.
 */
export function zoneChoice({ to, options }: ZoneChangeError, hasVenue: boolean): EditChoice {
	const [clock, instant] = [options.clock, options.instant].map(venueClock);
	const dayAfter = venueDay(options.instant) > venueDay(options.clock) ? ' the next day' : '';
	const dayBefore = venueDay(options.instant) < venueDay(options.clock) ? ' the day before' : '';
	const moment = `${instant} ${zoneCity(to)}${dayAfter}${dayBefore}`;
	const lead = hasVenue ? `The venue is in ${to}.` : `The times move to ${to}.`;

	return {
		name: 'zoneChange',
		question: `${lead} Keep ${clock} on its clock (${clock} ${zoneCity(to)}), or keep the same moment (${moment})?`,
		options: [
			{ value: 'clock', label: `Keep ${clock} on its clock` },
			{ value: 'instant', label: 'Keep the same moment' }
		]
	};
}
