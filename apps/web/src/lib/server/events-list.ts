import { z } from 'zod';
import type { ListedEvent, Listing } from '#lib/events.ts';
import { distanceKm, type LatLng } from '#lib/geo.ts';
import { eventPagePath, spacePagePath } from '#lib/space-paths.ts';
import type { ShowView } from '#lib/types.ts';
import { parseJson } from './db';
import type { EventView } from './services/events';

type Services = App.Locals['app'];

/** The most rows a page of the Events list has: the directory's page of shows. */
const PAGE = 30;

/**
 * Where the Events list's next page starts: the directory's page its shows continue on, how many of that page's shows
 * were already listed, or that none are left (`done`), and the last event listed.
 */
const pageCursor = z.object({
	show: z.string().optional(),
	skip: z.number().int().nonnegative(),
	done: z.boolean().optional(),
	after: z.object({ at: z.number(), uri: z.string() }).optional()
});

type PageCursor = z.infer<typeof pageCursor>;

const startOf = (item: Listing) => Date.parse('kind' in item ? item.startsAt : item.record.startsAt);

/** An event as lists of upcoming ones show it, `near` a place when the list is. Null when its party is gone. */
export async function listedEvent(
	app: Pick<Services, 'parties'>,
	{ uri, space, rkey, record, venue, standing }: EventView,
	near?: LatLng
): Promise<ListedEvent | null> {
	const party = await app.parties.bySpace(space);

	// A listed event always has a date and a venue (EventService.listed).
	if (!party || !venue || !record.startsAt) return null;
	const { name } = party.basics;
	const { geo, address } = venue;
	const place = geo && { lat: Number(geo.latitude), lng: Number(geo.longitude) };

	return {
		kind: 'event',
		uri,
		href: eventPagePath(party.skey, rkey, record.name),
		name: record.name,
		startsAt: record.startsAt,
		...(record.endsAt && { endsAt: record.endsAt }),
		status: record.status,
		...(record.tickets && { tickets: record.tickets }),
		party: { name, href: spacePagePath('party', party.skey, name) },
		venue: {
			name: venue.name,
			href: spacePagePath('venue', venue.skey, venue.name),
			...(address.locality && { locality: address.locality })
		},
		confirmed: standing.confirmed,
		...(near && place && { distanceKm: distanceKm(near, place) })
	};
}

/** The events listed after `after`, soonest first, within `radiusKm` of `near` when both are given. */
async function eventsAfter(
	app: Pick<Services, 'events' | 'parties'>,
	{ after, near, radiusKm }: { after?: PageCursor['after']; near?: LatLng; radiusKm?: number }
): Promise<ListedEvent[]> {
	const later = (await app.events.listed()).filter((event) => {
		const at = Date.parse(event.record.startsAt ?? '');

		return !after || at > after.at || (at === after.at && event.uri > after.uri);
	});

	const listed = await Promise.all(later.map((event) => listedEvent(app, event, near)));

	return listed.filter(
		(event): event is ListedEvent =>
			!!event && (!near || !radiusKm || (event.distanceKm !== undefined && event.distanceKm <= radiusKm))
	);
}

/**
 * A page of the Events list: upcoming shows from the directory and, with `events`, the parties' events listed now,
 * merged soonest first, in the area `near` and `radiusKm` give. A page ends early where its shows run out while the
 * directory has more, since its next page could start before the next event.
 */
export async function upcomingListings(
	app: Pick<Services, 'events' | 'parties'> & { shows: Pick<Services['shows'], 'upcoming'> },
	query: { near?: LatLng; radiusKm?: number; cursor?: string; events: boolean }
): Promise<{ items: Listing[]; cursor?: string }> {
	const { near, radiusKm } = query;
	const decoded = query.cursor && Buffer.from(query.cursor, 'base64url').toString();
	const from = pageCursor.safeParse(parseJson<unknown>(decoded, null)).data ?? { skip: 0 };

	const shows: { items: ShowView[]; cursor?: string } = from.done
		? { items: [] }
		: await app.shows.upcoming({ near, radiusKm, cursor: from.show, limit: PAGE });

	const ahead = shows.items.slice(from.skip);
	const events = query.events ? await eventsAfter(app, { after: from.after, near, radiusKm }) : [];
	const items: Listing[] = [];
	let [s, e] = [0, 0];

	while (items.length < PAGE) {
		const show = ahead.at(s);
		const event = shows.cursor && !show ? undefined : events.at(e);

		if (event && (!show || startOf(event) < startOf(show))) {
			items.push(event);
			e++;
		} else if (show) {
			items.push(show);
			s++;
		} else break;
	}

	const showsLeft = s < ahead.length;
	const lastEvent = items.findLast((item) => 'kind' in item);

	const next: PageCursor = {
		...(showsLeft
			? { show: from.show, skip: from.skip + s }
			: shows.cursor
				? { show: shows.cursor, skip: 0 }
				: { skip: 0, done: true }),
		after: lastEvent ? { at: startOf(lastEvent), uri: lastEvent.uri } : from.after
	};

	const isMore = showsLeft || !!shows.cursor || e < events.length;

	return { items, ...(isMore && { cursor: Buffer.from(JSON.stringify(next)).toString('base64url') }) };
}
