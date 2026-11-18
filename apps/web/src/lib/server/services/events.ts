import { timingSafeEqual } from 'node:crypto';
import { TID } from '@atproto/common-web';
import type { Selectable } from 'kysely';
import { toE6 } from '#lib/geo.ts';
import { COLLECTION } from '#lib/nsid.ts';
import { EVENT_STATUS } from '#lib/shows.ts';
import type { EventCredit, EventRecord, Location, Viewer } from '#lib/types.ts';
import { isWebUrl } from '#lib/web-url.ts';
import { isValidRecord, validateRecord } from '../atproto/lexicons';
import { parseSpaceRecordUri, parseSpaceUri, sharedSpace, sharedSpaceKind, spaceRecordUri } from '../atproto/spaces';
import { newToken } from '../auth/session';
import { parsePerformerUri } from '../performer-uri';
import { type Core, NotFoundError, UserError } from '../core';
import { nowIso } from '../db';
import type { EventTable } from '../db/schema';
import { atVenue, isTimeZone, nextDay, venueEnd, venueTime, zoneOf } from '../venue-time';
import { savedChanges } from './diff';
import type { PartyService } from './parties';
import type { PerformerService } from './performer-registry';
import type { ProfileService } from './profiles';
import { type PageInput, type SharedSpaces, STALE_PAGE } from './shared-spaces';
import type { EventStanding, VenueRules } from './venue-rules';
import type { VenueService, VenueView } from './venues';

/**
 * When saving would move an event's times onto another clock (a venue picked in another time zone than the draft's,
 * another venue, or a new time zone for the draft): `clock` reads the times as entered on the new clock, and `instant`
 * keeps the moments they were, written on the new clock.
 */
export type ZoneChange = 'clock' | 'instant';

/**
 * An event as its party's people fill it in: times are on the venue's clock, or while there's no venue, the draft's
 * time zone's, and set times are on the night's date. A draft needs only a name; publishing needs a date, a venue and
 * a published party too.
 */
export type EventInput = {
	name: string;
	description?: string;
	/** YYYY-MM-DD and HH:MM, on the event's clock. An end at or before the start is the next day. */
	date?: string;
	start?: string;
	end?: string;
	/** The venue's space. */
	venue?: string;
	/**
	 * The IANA time zone a draft without a venue keeps its times on. Left out, it stays as saved, or a new draft gets its
	 * party's area's zone, else its creator's, else UTC. A venue's clock replaces it.
	 */
	timeZone?: string;
	/** What to do when saving moves the times onto another clock; without it, such a save is refused (ZoneChangeError). */
	zoneChange?: ZoneChange;
	tickets?: string;
	audience: 'public' | 'private';
	/** Each performer's profile or act listing URI, with set times on the venue's clock. */
	lineup?: { performer: string; start?: string; end?: string }[];
	/** Whether this save publishes it. A draft stays one until a save publishes it. */
	publish: boolean;
};

/** Saving an event: its form, and the version of the event the form was opened on. */
export type EventPageInput = EventInput & Pick<PageInput, 'swap'>;

export type EventView = {
	uri: string;
	/** The party's space. */
	space: string;
	rkey: string;
	record: EventRecord;
	cid: string;
	venue: VenueView | null;
	approved: boolean;
	/**
	 * How the venue shows it: listed or not, and Confirmed or waiting for the venue. A draft or a private event is never
	 * listed.
	 */
	standing: EventStanding;
};

/** What an event's form fills in, which every save rebuilds. */
type EventFields = Omit<EventRecord, 'status' | 'linkToken' | 'publishedAt' | 'createdBy' | 'createdAt' | 'updatedAt'>;

/** An event's times and its lineup's, on one clock. */
type Times = { startsAt?: string; endsAt?: string; lineup: EventCredit[] };

/**
 * Refuses a save that would move the event's times onto another clock without saying how (`zoneChange`). `options`
 * has the start each choice would save, on the new clock, for the editor to ask with.
 */
export class ZoneChangeError extends UserError {
	constructor(
		readonly from: string,
		readonly to: string,
		readonly options: Record<ZoneChange, string>
	) {
		super(`The times move from ${from} to ${to}. Keep them as entered, or at the same moments?`);
		this.name = 'ZoneChangeError';
	}
}

/** The venue's time zone, from its coordinates, or null when it has none yet. */
export const venueZone = ({ geo }: VenueView) =>
	geo ? zoneOf({ latE6: toE6(Number(geo.latitude)), lngE6: toE6(Number(geo.longitude)) }) : null;

/** A location's time zone, or null when it has no coordinates (typed by hand, not picked from the city search). */
const locationZone = (location: Location | undefined) =>
	location?.latE6 !== undefined && location.lngE6 !== undefined
		? zoneOf({ latE6: location.latE6, lngE6: location.lngE6 })
		: null;

/** The night's times and the lineup's from what was entered, on `zone`'s clock. */
function readTimes(input: EventInput, date: string, start: string, zone: string): Times {
	const lineup = (input.lineup ?? []).map(({ performer, start: set, end }): EventCredit => {
		if (!set) return { performer };
		// A set that starts before the night does is after midnight.
		const day = set < start ? nextDay(date) : date;

		return { performer, startsAt: venueTime(day, set, zone), ...(end && { endsAt: venueEnd(day, set, end, zone) }) };
	});

	return {
		startsAt: venueTime(date, start, zone),
		...(input.end && { endsAt: venueEnd(date, start, input.end, zone) }),
		lineup
	};
}

/** The same moments as `times`, written on `zone`'s clock. */
const onClock = ({ startsAt, endsAt, lineup }: Times, zone: string): Times => ({
	...(startsAt && { startsAt: atVenue(startsAt, zone) }),
	...(endsAt && { endsAt: atVenue(endsAt, zone) }),
	lineup: lineup.map((credit) => ({
		performer: credit.performer,
		...(credit.startsAt && { startsAt: atVenue(credit.startsAt, zone) }),
		...(credit.endsAt && { endsAt: atVenue(credit.endsAt, zone) })
	}))
});

/** A datetime's instant, so the same moment on two clocks compares equal. */
const instant = (value: string | undefined) => (value === undefined ? undefined : Date.parse(value));

/** Every moment in `times`, to tell whether two readings land on the same ones. */
const moments = ({ startsAt, endsAt, lineup }: Times) =>
	[startsAt, endsAt, ...lineup.flatMap((credit) => [credit.startsAt, credit.endsAt])].map(instant).join();

const isPrivate = (record: Pick<EventRecord, 'audience'>) => record.audience !== 'public';

/** Whether `token` is the one a private event's link carries, compared in constant time. */
function sameToken(stored: string | undefined, token: string | null | undefined): boolean {
	if (!stored || !token) return false;
	const [a, b] = [Buffer.from(stored), Buffer.from(token)];

	return a.length === b.length && timingSafeEqual(a, b);
}

/**
 * A party's events, each a root-written record in the party's space. People with events.manage create, edit, publish
 * and cancel them; people with venue.approve at the venue approve them, as a record in the venue's own space. An event
 * starts as a draft unless it's published as it's created, and a draft is only its party's people's and staff's to
 * see. How the venue shows a published event follows its rules (VenueRules): its mode, its approval, and whether it
 * blocks the party or a performer.
 */
export class EventService {
	constructor(
		private readonly core: Core,
		private readonly sharedSpaces: SharedSpaces,
		private readonly venues: Pick<VenueService, 'bySpace'>,
		private readonly rules: VenueRules,
		private readonly performers: Pick<PerformerService, 'byRef'>,
		private readonly parties: Pick<PartyService, 'bySpace'>,
		private readonly profiles: Pick<ProfileService, 'get'>
	) {}

	/** Creates an event in the party's space, and returns its URI. Needs events.manage. A private one gets a link token. */
	async create(actor: Viewer, party: string, input: EventInput): Promise<string> {
		let uri = '';

		await this.sharedSpaces.change(party, async (live) => {
			if (live.kind !== 'party') throw new NotFoundError('That party does not exist');
			const actingAs = this.sharedSpaces.authorize(actor, live, 'events.manage');
			const now = nowIso();
			const fields = await this.fields(party, input, input.publish, actor.did);

			const record = validateRecord(COLLECTION.event, {
				...fields,
				status: EVENT_STATUS.scheduled,
				...(isPrivate(input) && { linkToken: newToken() }),
				...(input.publish && { publishedAt: now }),
				createdBy: actor.did,
				createdAt: now,
				updatedAt: now
			});

			const rkey = TID.nextStr();
			uri = spaceRecordUri(party, live.root.did, COLLECTION.event, rkey);
			await live.root.putSpaceRecord({ space: party, collection: COLLECTION.event, rkey, record });
			await this.sharedSpaces.index({ space: party, collection: COLLECTION.event, rkey, value: record });

			const summary = `Created ${fields.name}${input.publish ? '' : ' as a draft'}`;

			await this.sharedSpaces.record(actor, actingAs, 'event.create', party, summary, { event: uri });
		});

		this.core.logger.info('created an event', { uri, published: input.publish });

		return uri;
	}

	/**
	 * Saves an event over the version its form was opened on, and publishes it when asked. Needs events.manage. Moving a
	 * published event's start marks it rescheduled; making it private gives it a link token, and making it public drops
	 * it.
	 */
	async update(actor: Viewer, uri: string, input: EventPageInput): Promise<void> {
		await this.change(actor, uri, input.swap, 'event.update', async (current, party) => {
			const publishedAt = current.publishedAt ?? (input.publish ? nowIso() : undefined);
			const fields = await this.fields(party, input, !!publishedAt, current.createdBy, current);
			// Only a published event's start was seen by anyone, so only moving that one reschedules it.
			const moved = !!current.publishedAt && instant(fields.startsAt) !== instant(current.startsAt);
			const status = current.status === EVENT_STATUS.cancelled || !moved ? current.status : EVENT_STATUS.rescheduled;

			return {
				// Built from the form, so a field it left empty goes; who made it and when stay.
				record: {
					...fields,
					status,
					...(isPrivate(input) && { linkToken: current.linkToken ?? newToken() }),
					...(publishedAt && { publishedAt }),
					createdBy: current.createdBy,
					createdAt: current.createdAt,
					updatedAt: current.updatedAt
				},
				summary: `${publishedAt === current.publishedAt ? 'Updated' : 'Published'} ${fields.name}`
			};
		});
	}

	/** Cancels an event, which stays listed as cancelled. Needs events.manage. */
	async cancel(actor: Viewer, uri: string, swap: string): Promise<void> {
		await this.change(actor, uri, swap, 'event.update', async (current) => ({
			record: { ...current, status: EVENT_STATUS.cancelled },
			summary: `Cancelled ${current.name}`
		}));
	}

	/**
	 * Gives a private event a new link token, so its old link stops opening it (ADR 0015). Needs events.manage. It goes
	 * over whatever version is current, since nothing else in the event changes.
	 */
	async resetLink(actor: Viewer, uri: string): Promise<void> {
		await this.change(actor, uri, null, 'event.resetLink', async (current) => {
			if (!isPrivate(current)) throw new UserError('Only a private event has a link to reset');

			return { record: { ...current, linkToken: newToken() }, summary: `Reset the link to ${current.name}` };
		});

		this.core.logger.info("reset a private event's link", { uri });
	}

	/** The venue confirms a published event naming it. Needs venue.approve at the event's venue. */
	async approve(actor: Viewer, uri: string): Promise<void> {
		const event = await this.get(uri);
		// A draft names no venue to anyone outside its party yet.
		const venue = event?.record.publishedAt && event.record.venue;

		if (!event || !venue) throw new NotFoundError('That event does not exist');

		await this.sharedSpaces.change(venue, async (live) => {
			const actingAs = this.sharedSpaces.authorize(actor, live, 'venue.approve');

			if (await this.approved(venue, uri)) return;

			const record = validateRecord(COLLECTION.venueApproval, {
				event: uri,
				approvedBy: actor.did,
				createdAt: nowIso()
			});

			const rkey = TID.nextStr();

			await live.root.putSpaceRecord({ space: venue, collection: COLLECTION.venueApproval, rkey, record });
			await this.sharedSpaces.index({ space: venue, collection: COLLECTION.venueApproval, rkey, value: record });
			await this.sharedSpaces.record(actor, actingAs, 'event.approve', venue, `Confirmed ${event.record.name}`, {
				event: uri
			});
		});
	}

	/** Any event, drafts and private ones too, for its party's people and staff. */
	async get(uri: string): Promise<EventView | null> {
		const row = await this.core.db.selectFrom('event').selectAll().where('uri', '=', uri).executeTakeFirst();

		return row ? this.toView(row) : null;
	}

	/**
	 * The event as a visitor opens it from a link, signed in or not: a published public one for anyone, a published
	 * private one only with its link's token (ADR 0015). Anything else is as if it weren't there.
	 */
	async open(uri: string, token: string | null): Promise<EventView | null> {
		const event = await this.get(uri);

		if (!event?.record.publishedAt) return null;

		if (isPrivate(event.record) && !sameToken(event.record.linkToken, token)) return null;

		return event;
	}

	/** A party's events, drafts and private ones too, soonest first and undated drafts last. */
	async ofParty(party: string): Promise<EventView[]> {
		const rows = await this.core.db
			.selectFrom('event')
			.selectAll()
			.where('spaceUri', '=', party)
			.orderBy((eb) => eb('startsAt', 'is', null))
			.orderBy('startsAt')
			.execute();

		return Promise.all(rows.map((row) => this.toView(row)));
	}

	/**
	 * The upcoming published events naming the venue that are waiting for the venue, soonest first, for the people who
	 * approve them.
	 */
	async waiting(venue: string): Promise<EventView[]> {
		const rows = await this.core.db
			.selectFrom('event')
			.selectAll()
			.where('venueSpace', '=', venue)
			.where('publishedAt', 'is not', null)
			.where('status', '!=', EVENT_STATUS.cancelled)
			.where('startsAt', '>=', nowIso())
			.where((eb) =>
				eb.not(
					eb.exists(eb.selectFrom('venueApproval').select('rkey').whereRef('venueApproval.eventUri', '=', 'event.uri'))
				)
			)
			.orderBy('startsAt')
			.execute();

		return Promise.all(rows.map((row) => this.toView(row)));
	}

	/**
	 * Runs `next` on the event's current record, then writes what it returns, over the version `swap` names unless it's
	 * null. The audit entry has what changed, field by field, but never the link token.
	 */
	private async change(
		actor: Viewer,
		uri: string,
		swap: string | null,
		action: 'event.update' | 'event.resetLink',
		next: (current: EventRecord, party: string) => Promise<{ record: EventRecord; summary: string }>
	): Promise<void> {
		const ref = parseSpaceRecordUri(uri);
		const party = ref && sharedSpaceKind(ref.type) === 'party' ? sharedSpace(ref.authority, 'party', ref.skey) : null;

		if (!ref || !party || ref.collection !== COLLECTION.event) throw new NotFoundError('That event does not exist');

		await this.sharedSpaces.change(party, async (live) => {
			const actingAs = this.sharedSpaces.authorize(actor, live, 'events.manage');

			const stored = await live.root.getSpaceRecord({
				space: party,
				repo: live.root.did,
				collection: COLLECTION.event,
				rkey: ref.rkey
			});

			if (!stored || !isValidRecord<EventRecord>(COLLECTION.event, stored.value))
				throw new NotFoundError('That event does not exist');

			if (swap !== null && stored.cid !== swap) throw new UserError(STALE_PAGE, 409);
			const { record, summary } = await next(stored.value, party);
			const value = validateRecord(COLLECTION.event, { ...record, updatedAt: nowIso() });
			const changes = savedChanges(stored.value, value).filter((change) => change.field !== 'linkToken');

			await live.root.putSpaceRecord({ space: party, collection: COLLECTION.event, rkey: ref.rkey, record: value });
			await this.sharedSpaces.index({ space: party, collection: COLLECTION.event, rkey: ref.rkey, value });
			await this.sharedSpaces.record(actor, actingAs, action, party, summary, { event: uri }, changes);
		});
	}

	/**
	 * The record's fields from what was filled in, checked: real performers, a web link, a known time zone, a venue with
	 * a location for any times on its clock, and when it's `published`, a date, a venue and a published party, so it
	 * never shows a draft party. `current` is the event as saved, for an update: times entered on its clock that would
	 * land on other moments on the new one need `input.zoneChange`.
	 */
	private async fields(
		party: string,
		input: EventInput,
		published: boolean,
		creator: string,
		current?: EventRecord
	): Promise<EventFields> {
		const name = input.name.trim();

		if (!name) throw new UserError('Give the event a name');
		const venue = input.venue ? await this.venues.bySpace(input.venue) : null;

		if (input.venue && !venue) throw new UserError('Pick a venue');

		if (published) {
			if (!(input.date && venue)) throw new UserError('Add a date and a venue to publish it');

			if (!(await this.parties.bySpace(party))?.publishedAt) throw new UserError('Publish the party first');
		}

		if (input.tickets && !isWebUrl(input.tickets))
			throw new UserError('Use a full web address for tickets, starting with https://');

		if (input.timeZone && !isTimeZone(input.timeZone)) throw new UserError('Pick a time zone from the list');
		const { date, start } = input;

		for (const set of input.lineup ?? []) {
			if (!(await this.isPerformer(set.performer)))
				throw new UserError('Someone on the lineup is no longer on the site');

			if (set.start && !date) throw new UserError("Add the night's date before set times");
		}

		// The venue's clock, or while there's no venue, the draft's own time zone.
		const draftZone = venue ? null : (input.timeZone ?? current?.timeZone ?? (await this.homeZone(party, creator)));
		const zone = venue ? venueZone(venue) : draftZone;
		let times: Times = { lineup: (input.lineup ?? []).map(({ performer }) => ({ performer })) };

		if (date || start) {
			if (!date || !start) throw new UserError('Add the date and when it starts');

			if (!zone) throw new UserError("That venue has no location yet, so its clock isn't known");
			times = readTimes(input, date, start, zone);
			// The times were entered on the clock the event was on, so moving it to another must not shift them unasked.
			const before = current && (await this.clockOf(current));

			if (before && before !== zone) {
				const kept = onClock(readTimes(input, date, start, before), zone);

				if (moments(kept) !== moments(times)) {
					if (!input.zoneChange)
						throw new ZoneChangeError(before, zone, {
							clock: venueTime(date, start, zone),
							instant: atVenue(venueTime(date, start, before), zone)
						});

					if (input.zoneChange === 'instant') times = kept;
				}
			}
		}

		const { lineup, ...when } = times;
		const description = input.description?.trim();

		return {
			name,
			...(description && { description }),
			...when,
			...(venue && { venue: venue.space }),
			...(draftZone && { timeZone: draftZone }),
			...(input.tickets && { tickets: input.tickets }),
			audience: input.audience,
			...(lineup.length > 0 && { lineup })
		};
	}

	/** The time zone an event's times are on: its venue's, or while it has none, its own, when it has one. */
	private async clockOf(event: EventRecord): Promise<string | null | undefined> {
		if (!event.venue) return event.timeZone;
		const venue = await this.venues.bySpace(event.venue);

		return venue && venueZone(venue);
	}

	/** The time zone a new venue-less draft starts on: its party's area's, else its creator's profile's, else UTC. */
	async homeZone(party: string, creator: string): Promise<string> {
		const area = locationZone((await this.parties.bySpace(party))?.basics.location);

		if (area) return area;

		return locationZone((await this.profiles.get(creator))?.record.basics.location) ?? 'UTC';
	}

	/** Whether `uri` names a DJ's profile, or an act that's on the site. Projects don't go on lineups. */
	private async isPerformer(uri: string): Promise<boolean> {
		const ref = parsePerformerUri(this.core.rootDid, uri);

		if (ref?.kind === 'dj') return true;

		return ref?.kind === 'act' && (await this.performers.byRef(ref))?.kind === 'act';
	}

	private async approved(venue: string, uri: string): Promise<boolean> {
		const row = await this.core.db
			.selectFrom('venueApproval')
			.select('rkey')
			.where('spaceUri', '=', venue)
			.where('eventUri', '=', uri)
			.executeTakeFirst();

		return !!row;
	}

	private async toView(row: Selectable<EventTable>): Promise<EventView> {
		// SAFETY: indexEvent is the only writer of the event table, and stores a record valid by the event lexicon.
		const record = JSON.parse(row.record) as EventRecord;
		// SAFETY: indexEvent only takes events from a party space under the root.
		const party = parseSpaceUri(row.spaceUri)!;

		const subjects = [
			`at://${this.core.rootDid}/${COLLECTION.listing}/${party.skey}`,
			...(record.lineup ?? []).map((credit) => credit.performer)
		];

		const venue = row.venueSpace;
		const approved = venue !== null && (await this.approved(venue, row.uri));
		const rules = venue === null ? null : await this.rules.event(venue, subjects, approved);
		const listed = !!rules?.listed && row.publishedAt !== null && !isPrivate(record);

		return {
			uri: row.uri,
			space: row.spaceUri,
			rkey: row.rkey,
			record,
			cid: row.cid,
			venue: venue === null ? null : await this.venues.bySpace(venue),
			approved,
			standing: { listed, confirmed: !!rules?.confirmed }
		};
	}
}
