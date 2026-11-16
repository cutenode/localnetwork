import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Listing } from '#lib/events.ts';
import { EVENT_STATUS } from '#lib/shows.ts';
import { NEON_TIDE } from '#lib/testing/link-matrix.ts';
import type { ShowView } from '#lib/types.ts';
import { parseSpaceRecordUri, parseSpaceUri } from './atproto/spaces';
import { upcomingListings } from './events-list';
import type { EventInput } from './services/events';
import type { ShowQuery } from './services/shows';
import type { VenueView } from './services/venues';
import { createTestServices } from './testing/core';

const ADMIN = 'did:plc:admin';

const HOST = 'did:plc:host';

const BERLIN = { lat: 52.52, lng: 13.405 };

let t: Awaited<ReturnType<typeof createTestServices>>;

let party: string;

let tresor: VenueView;

let hafen: VenueView;

/** A show announced for Neon Tide, starting at `startsAt`. */
const show = (rkey: string, startsAt: string): ShowView => ({
	uri: `at://did:plc:neon/computer.localnetwork.show/${rkey}`,
	cid: 'c',
	did: 'did:plc:neon',
	rkey,
	project: NEON_TIDE,
	record: {
		project: NEON_TIDE.uri,
		event: { uri: `at://did:plc:neon/community.lexicon.calendar.event/${rkey}`, cid: 'e' },
		name: rkey,
		startsAt,
		location: { city: 'Berlin', country: 'DE' },
		status: EVENT_STATUS.scheduled,
		createdAt: '2026-10-01T00:00:00Z'
	}
});

/** The directory with `shows` upcoming, paged as listShows pages them: its cursor is where the next page starts. */
const directory = (shows: ShowView[], queries: ShowQuery[] = []) => ({
	upcoming: async (query: ShowQuery) => {
		queries.push(query);
		const from = Number(query.cursor ?? 0);
		const to = from + (query.limit ?? 30);

		return { items: shows.slice(from, to), ...(to < shows.length && { cursor: String(to) }) };
	}
});

/** The `day`th of November 2026 (or later, past its 30th) as YYYY-MM-DD. */
const dayOf = (day: number) => new Date(Date.UTC(2026, 10, day)).toISOString().slice(0, 10);

const create = async (overrides: Partial<EventInput> = {}) =>
	t.events.create(await t.viewer(HOST), party, {
		name: 'Winter Opening',
		date: '2026-11-20',
		start: '23:00',
		venue: tresor.space,
		audience: 'public',
		publish: true,
		...overrides
	});

/** Every page of the list, following each one's cursor to the end. */
async function everyPage(app: Parameters<typeof upcomingListings>[0], query: { events: boolean }) {
	const pages: Listing[][] = [];
	let cursor: string | undefined;

	do {
		const page = await upcomingListings(app, { ...query, cursor });
		pages.push(page.items);
		cursor = page.cursor;
	} while (cursor);

	return pages;
}

beforeEach(async () => {
	vi.useFakeTimers({ toFake: ['Date'] });
	vi.setSystemTime(new Date('2026-11-13T12:00:00Z'));
	t = await createTestServices({ admins: [ADMIN] });
	const admin = await t.viewer(ADMIN);
	party = await t.parties.start(await t.viewer(HOST), {
		basics: { name: 'Night Shift' },
		avatar: 'keep',
		publish: true
	});
	tresor = await t.venues.add(admin, {
		name: 'Tresor',
		address: { country: 'DE', locality: 'Berlin' },
		geo: { latitude: '52.51', longitude: '13.42' }
	});
	hafen = await t.venues.add(admin, {
		name: 'Hafenklang',
		address: { country: 'DE', locality: 'Hamburg' },
		geo: { latitude: '53.545', longitude: '9.96' }
	});
});

afterEach(async () => {
	vi.useRealTimers();
	await t.audit.idle();
	await t.db.destroy();
});

describe('upcomingListings', () => {
	it("merges the parties' listed events into the shows soonest first, each with its pages and standing", async () => {
		const winter = await create({ end: '06:00', tickets: 'https://tickets.example/winter' });
		const harbour = await create({ name: 'Harbour', date: '2026-11-21', venue: hafen.space });
		const before = show('before', '2026-11-19T23:00:00+01:00');
		const between = show('between', '2026-11-21T20:00:00+01:00');
		const key = parseSpaceUri(party)!.skey;
		const rkey = parseSpaceRecordUri(winter)!.rkey;

		const { items, cursor } = await upcomingListings({ ...t, shows: directory([before, between]) }, { events: true });

		expect(items.map((item) => item.uri)).toEqual([before.uri, winter, between.uri, harbour]);
		expect(items[1]).toEqual({
			kind: 'event',
			uri: winter,
			href: `/party/${key}/${rkey}/winter-opening`,
			name: 'Winter Opening',
			startsAt: '2026-11-20T23:00:00+01:00',
			endsAt: '2026-11-21T06:00:00+01:00',
			status: EVENT_STATUS.scheduled,
			tickets: 'https://tickets.example/winter',
			party: { name: 'Night Shift', href: `/party/${key}/night-shift` },
			venue: { name: 'Tresor', href: `/venue/${tresor.skey}/tresor`, locality: 'Berlin' },
			confirmed: false
		});
		expect(cursor).toBeUndefined();
	});

	it('lists only shows without `events`, and only what is within the radius near a place', async () => {
		const winter = await create();
		await create({ name: 'Harbour', venue: hafen.space });
		const queries: ShowQuery[] = [];
		const shows = directory([show('only', '2026-11-19T23:00:00+01:00')], queries);

		const without = await upcomingListings({ ...t, shows }, { events: false });
		const near = await upcomingListings({ ...t, shows }, { events: true, near: BERLIN, radiusKm: 25 });

		expect(without.items.map((item) => item.uri)).toEqual(['at://did:plc:neon/computer.localnetwork.show/only']);
		expect(near.items.map((item) => item.uri)).toEqual(['at://did:plc:neon/computer.localnetwork.show/only', winter]);
		expect(near.items[1]).toMatchObject({ distanceKm: expect.closeTo(1.5, 0) });
		expect(queries.at(-1)).toMatchObject({ near: BERLIN, radiusKm: 25 });
	});

	it('pages through every show and event once, in order, a page of at most 30 at a time', async () => {
		// 45 shows, one every evening from 14 November, and an event at 1:00 every fifth night, between two of them.
		const shows = Array.from({ length: 45 }, (_, day) =>
			show(`s${day}`, new Date(Date.UTC(2026, 10, 14 + day, 21)).toISOString())
		);

		const events: string[] = [];

		for (let night = 0; night < 45; night += 5)
			events.push(await create({ name: `Night ${night}`, date: dayOf(15 + night), start: '01:00' }));

		const pages = await everyPage({ ...t, shows: directory(shows) }, { events: true });
		const listed = pages.flat();
		const starts = listed.map((item) => Date.parse('kind' in item ? item.startsAt : item.record.startsAt));

		expect(pages.every((page) => page.length > 0 && page.length <= 30)).toBe(true);
		expect(listed).toHaveLength(shows.length + events.length);
		expect(new Set(listed.map((item) => item.uri)).size).toBe(listed.length);
		expect(starts).toEqual(starts.toSorted((a, b) => a - b));
	});

	it('keeps its place in a page of shows across pages that are mostly events', async () => {
		// One show in November, then a month of nightly events, then 30 shows from January: the directory's first page
		// of 30 shows is listed over three pages.
		const shows = [
			show('first', '2026-11-14T23:00:00+01:00'),
			...Array.from({ length: 30 }, (_, day) => show(`s${day}`, new Date(Date.UTC(2027, 0, 1 + day, 21)).toISOString()))
		];

		for (let day = 15; day < 46; day++) await create({ name: `Night ${day}`, date: dayOf(day) });

		const pages = await everyPage({ ...t, shows: directory(shows) }, { events: true });
		const listed = pages.flat();

		expect(pages.map((page) => page.length)).toEqual([30, 30, 1, 1]);
		expect(listed).toHaveLength(shows.length + 31);
		expect(new Set(listed.map((item) => item.uri)).size).toBe(listed.length);
	});

	it('goes on with events once no shows are left, and starts over from a cursor it cannot read', async () => {
		const shows = directory([show('first', '2026-11-14T23:00:00+01:00')]);

		for (let day = 15; day < 46; day++) await create({ name: `Night ${day}`, date: dayOf(day) });

		const pages = await everyPage({ ...t, shows }, { events: true });
		const restart = await upcomingListings({ ...t, shows }, { events: true, cursor: 'not a cursor' });

		expect(pages.map((page) => page.length)).toEqual([30, 2]);
		expect(pages[0][0].uri).toBe('at://did:plc:neon/computer.localnetwork.show/first');
		expect(restart.items).toEqual(pages[0]);
	});
});
