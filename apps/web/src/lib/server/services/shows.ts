import { TID } from '@atproto/common-web';
import type { JsonObject } from '@atproto/lex-json';
import { z } from 'zod';
import { type LatLng, toE6 } from '#lib/geo.ts';
import { CALENDAR_EVENT, COLLECTION } from '#lib/nsid.ts';
import { EVENT_STATUS, type ShowFormValues, venueClock, venueDay } from '#lib/shows.ts';
import type { Location, ProjectView, ShowRecord, ShowView, StrongRef, Viewer } from '#lib/types.ts';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import { recordCid, xrpcErrorName } from '../atproto/repo-client';
import { parsePublicRecordUri } from '../atproto/spaces';
import { type Core, ForbiddenError, NotFoundError, UserError } from '../core';
import { nowIso } from '../db';
import type { DirectoryShow, HappyViewClient } from '../happyview/client';
import { awaitIndexed, INDEX_WAIT_MS, type IndexedRepo, reindex } from '../happyview/indexed';
import { atVenue, venueTime } from '../venue-time';
import { writeOwn } from './own-repo';
import type { ProjectService } from './projects';
import { actorRole } from './requests';

/** A venue's city, with the coordinates its clock is found from. */
export type Venue = Location & { latE6: number; lngE6: number };

/** A show as the announce form gives it. Times are on the venue's clock; an end at or before the start is the next day. */
export type ShowInput = {
	name: string;
	/** YYYY-MM-DD */
	date: string;
	/** HH:MM */
	start: string;
	end?: string;
	venue?: string;
	location: Venue;
	tickets?: string;
	cancelled: boolean;
};

/** An edit of the show at `rkey`, made over the versions of it and its calendar event (null: none) the form showed. */
export type ShowEdit = { rkey: string; showCid: string; eventCid: string | null };

export type ShowQuery = {
	project?: string;
	follower?: string;
	near?: LatLng;
	radiusKm?: number;
	sort?: 'date' | 'distance';
	limit?: number;
	cursor?: string;
};

const REAUTH = 'Sign in again to announce shows';

const CHANGED = 'This show changed since you opened it. Reload and try again.';

const ADDRESS = 'community.lexicon.location.address';

const GEO = 'community.lexicon.location.geo';

/** The most upcoming shows a project's page lists. */
const PROJECT_SHOWS = 20;

const DAY_MS = 86_400_000;

/** What the announce form reads back from a calendar event, which another app may have written or changed. */
const eventSchema = z.object({
	name: z.string(),
	createdAt: z.string(),
	startsAt: z.string().optional(),
	endsAt: z.string().optional(),
	status: z.string().optional(),
	locations: z
		.array(
			z.object({
				$type: z.string().optional(),
				name: z.string().optional(),
				locality: z.string().optional(),
				region: z.string().optional(),
				country: z.string().optional(),
				latitude: z.string().optional(),
				longitude: z.string().optional()
			})
		)
		.optional(),
	uris: z.array(z.object({ uri: z.string() })).optional()
});

type EventRecord = z.infer<typeof eventSchema>;

function assertShowRecord(value: JsonObject): asserts value is JsonObject & ShowRecord {
	validateRecord(COLLECTION.show, value);
}

/** A record checked against its lexicon, or null when it doesn't fit. */
function readShow(value: JsonObject): ShowRecord | null {
	try {
		assertShowRecord(value);

		return value;
	} catch (error) {
		if (error instanceof RecordValidationError) return null;
		throw error;
	}
}

function readEvent(value: JsonObject): EventRecord | null {
	try {
		return eventSchema.safeParse(validateRecord(CALENDAR_EVENT, value)).data ?? null;
	} catch (error) {
		if (error instanceof RecordValidationError) return null;
		throw error;
	}
}

/** The day after `date` (YYYY-MM-DD). */
const nextDay = (date: string) => new Date(Date.parse(`${date}T00:00:00Z`) + DAY_MS).toISOString().slice(0, 10);

/** A calendar event as this app writes it, with the fields its show copies. */
type CalendarEvent = JsonObject & { startsAt: string; endsAt?: string; status: string };

/**
 * The `community.lexicon.calendar.event` for a show, over `previous` (the event as it is now, when editing) so what
 * other calendar apps added to it stays, unless it no longer fits the lexicon. A start that moves from the one `listed`
 * (the show's copy) marks it rescheduled, and it stays rescheduled.
 */
export function eventRecord(input: ShowInput, previous?: JsonObject | null, listed?: ShowRecord | null): CalendarEvent {
	const { location } = input;
	const before = previous ? readEvent(previous) : null;
	const startsAt = venueTime(input.date, input.start, location);
	const wasMoved = [before?.status, listed?.status].includes(EVENT_STATUS.rescheduled);
	// Compared as instants: the same moment can be written with different offsets.
	const moved = wasMoved || (!!listed && Date.parse(listed.startsAt) !== Date.parse(startsAt));
	const address: JsonObject = { $type: ADDRESS, locality: location.city, country: location.country };
	const locations = [address];

	if (input.venue) address.name = input.venue;

	if (location.region) address.region = location.region;

	const geo: JsonObject = {
		$type: GEO,
		latitude: String(location.latE6 / 1e6),
		longitude: String(location.lngE6 / 1e6)
	};

	if (input.venue) geo.name = input.venue;
	locations.push(geo);

	const event: CalendarEvent = {
		...(before ? previous : null),
		name: input.name,
		startsAt,
		mode: 'community.lexicon.calendar.event#inperson',
		status: input.cancelled ? EVENT_STATUS.cancelled : moved ? EVENT_STATUS.rescheduled : EVENT_STATUS.scheduled,
		locations,
		createdAt: before?.createdAt ?? nowIso()
	};

	delete event.endsAt;
	delete event.uris;

	if (input.end)
		event.endsAt = venueTime(input.end > input.start ? input.date : nextDay(input.date), input.end, location);

	if (input.tickets) event.uris = [{ uri: input.tickets, name: 'Tickets' }];

	return event;
}

/** The show record for a written event: the project, the event's version, and the copy of it listings read. */
export function showRecord(
	project: string,
	ref: StrongRef,
	input: ShowInput,
	event: CalendarEvent,
	createdAt = nowIso()
): ShowRecord {
	const record: ShowRecord = {
		project,
		event: ref,
		name: input.name,
		startsAt: event.startsAt,
		location: input.location,
		status: event.status,
		createdAt
	};

	if (event.endsAt) record.endsAt = event.endsAt;

	if (input.venue) record.venue = input.venue;

	if (input.tickets) record.tickets = input.tickets;

	return record;
}

/** The announce form's values for a show: its event as it is now, with the show's copy filling what the event lacks. */
function formValues(show: ShowRecord, event: EventRecord | null): Omit<ShowFormValues, 'showCid' | 'eventCid'> {
	const address = event?.locations?.find((place) => place.$type === ADDRESS);
	const geo = event?.locations?.find((place) => place.$type === GEO);
	const lat = Number(geo?.latitude);
	const lng = Number(geo?.longitude);
	const city = address?.locality;
	const country = address?.country?.toUpperCase();
	const fromEvent = !!city && country?.length === 2;
	const location: Location = fromEvent ? { city, region: address?.region, country } : { ...show.location };

	if (geo && Number.isFinite(lat) && Number.isFinite(lng))
		Object.assign(location, { latE6: toE6(lat), lngE6: toE6(lng) });
	else Object.assign(location, { latE6: show.location.latE6, lngE6: show.location.lngE6 });
	const { latE6, lngE6 } = location;
	// Without coordinates there's no clock to read times on; the form then asks for the city again anyway.
	const local = (time: string) => (latE6 === undefined || lngE6 === undefined ? time : atVenue(time, { latE6, lngE6 }));
	const startsAt = local(event?.startsAt ?? show.startsAt);
	const endsAt = event ? event.endsAt : show.endsAt;

	return {
		name: event?.name ?? show.name,
		date: venueDay(startsAt),
		start: venueClock(startsAt),
		end: endsAt ? venueClock(local(endsAt)) : '',
		venue: (fromEvent ? address?.name : show.venue) ?? '',
		city: location.city,
		region: location.region ?? '',
		country: location.country,
		latE6: latE6?.toString() ?? '',
		lngE6: lngE6?.toString() ?? '',
		tickets: (event ? event.uris?.[0]?.uri : show.tickets) ?? '',
		cancelled: (event?.status ?? show.status) === EVENT_STATUS.cancelled
	};
}

/**
 * Shows projects announce. Each is a `community.lexicon.calendar.event` in the announcing manager's own repo, so
 * calendar apps can read it, beside a `computer.localnetwork.show` with the same key that ties it to the project and
 * copies what listings need. HappyView indexes both (lua/computer/localnetwork/show.lua and
 * lua/community/lexicon/calendar/event.lua), answers every listing, and is where the app reads them back.
 */
export class ShowService {
	constructor(
		private readonly core: Core,
		private readonly projects: ProjectService,
		private readonly directory: HappyViewClient,
		private readonly indexed: IndexedRepo,
		private readonly indexWaitMs = INDEX_WAIT_MS
	) {}

	/** Upcoming shows, a page at a time. */
	async upcoming(query: ShowQuery): Promise<{ items: ShowView[]; cursor?: string }> {
		const { near } = query;

		const output = await this.directory.query('listShows', {
			project: query.project,
			follower: query.follower,
			latE6: near && toE6(near.lat),
			lngE6: near && toE6(near.lng),
			radiusKm: query.radiusKm && Math.round(query.radiusKm),
			sort: query.sort,
			limit: query.limit,
			cursor: query.cursor
		});

		return { items: output.shows.flatMap((show) => this.toView(show) ?? []), cursor: output.cursor };
	}

	async forProject(project: ProjectView): Promise<ShowView[]> {
		return (await this.upcoming({ project: project.uri, limit: PROJECT_SHOWS })).items;
	}

	/**
	 * A show `actor` announced for `project`, as the announce form's values, read from HappyView: the calendar event
	 * as it is now, since another app may have changed it. Null when there's no such show.
	 */
	async values(actor: Viewer, project: ProjectView, rkey: string): Promise<ShowFormValues | null> {
		const [show, event] = await this.stored(actor.did, rkey);
		const record = show && readShow(show.value);

		if (!show || record?.project !== project.uri) return null;

		return { ...formValues(record, event && readEvent(event.value)), showCid: show.cid, eventCid: event?.cid ?? '' };
	}

	/**
	 * Of `did`'s shows in `shows`, the ones whose calendar event changed after the show copied it, as HappyView has
	 * the events. An event it doesn't have, or a HappyView that's down, reads as no change.
	 */
	async edited(did: string, shows: ShowView[]): Promise<string[]> {
		const own = shows.filter((show) => show.did === did && parsePublicRecordUri(show.record.event.uri)?.did === did);
		const events = await this.indexed.getRecords(own.map((show) => show.record.event.uri));

		return own.flatMap((show) => {
			const event = events.get(show.record.event.uri);

			return event && event.cid !== show.record.event.cid ? [show.uri] : [];
		});
	}

	/**
	 * Announces a show, or makes `edit` to one `actor` announced: the calendar event and the show copying it. A new show
	 * is one write, so neither is ever left without the other. An edit only lands on the versions its form showed. The
	 * event's CID is worked out here, the way the PDS works it out.
	 */
	async save(actor: Viewer, project: ProjectView, input: ShowInput, edit?: ShowEdit): Promise<StrongRef> {
		if (!project.managers.includes(actor.did))
			throw new ForbiddenError("Only this project's managers can announce its shows");

		if (project.status !== 'active') throw new UserError('Only listed projects can announce shows');
		const key = edit?.rkey ?? TID.nextStr();
		const uri = `at://${actor.did}/${COLLECTION.show}/${key}`;
		const [show, event] = edit ? await this.stored(actor.did, edit.rkey) : [null, null];
		const listed = show && readShow(show.value);

		if (edit && listed?.project !== project.uri) throw new NotFoundError('That show does not exist');

		// The new event is built over the version HappyView has, so that has to be the one the form showed: a change
		// HappyView picked up while the form was open is refused here, before anything is written.
		if (edit && (show?.cid !== edit.showCid || (event?.cid ?? null) !== edit.eventCid)) {
			this.core.logger.info({ uri }, 'a show or its event changed since its form was opened');
			throw new UserError(CHANGED, 409);
		}

		const written = validateRecord(CALENDAR_EVENT, eventRecord(input, event?.value, listed));
		const eventRef = { uri: `at://${actor.did}/${CALENDAR_EVENT}/${key}`, cid: await recordCid(written) };
		const copy = showRecord(project.uri, eventRef, input, written, listed?.createdAt);
		const record = validateRecord(COLLECTION.show, JSON.parse(JSON.stringify(copy)));

		await writeOwn(this.core, actor.did, REAUTH, async (client) => {
			if (!edit)
				return client.applyWrites([
					{ action: 'create', collection: CALENDAR_EVENT, rkey: key, record: written },
					{ action: 'create', collection: COLLECTION.show, rkey: key, record }
				]);

			// applyWrites can't swap single records, so an edit writes them one at a time, the event first: other calendar
			// apps change only the event and this app's edits change both, so either trips the first swap, before anything
			// is written. A show that changed alone stops the edit with its copy behind the new event, which the project's
			// dashboard marks for the manager to review.
			try {
				await client.putRecord({
					collection: CALENDAR_EVENT,
					rkey: key,
					record: written,
					swapRecord: edit.eventCid
				});
				await client.putRecord({ collection: COLLECTION.show, rkey: key, record, swapRecord: edit.showCid });
			} catch (error) {
				if (xrpcErrorName(error) !== 'InvalidSwap') throw error;
				this.core.logger.info({ uri }, 'a show or its event changed on the PDS since HappyView served it');
				// Changed somewhere HappyView hasn't caught up with yet, so a reload would show the same old version.
				await Promise.all([
					this.indexed.reindex(actor.did, CALENDAR_EVENT),
					this.indexed.reindex(actor.did, COLLECTION.show)
				]);
				throw new UserError(CHANGED, 409);
			}
		});

		const cid = await recordCid(record);

		await Promise.all([
			this.awaitServed(actor.did, async () => (await this.served(uri))?.cid === cid),
			this.indexed.awaitWrite(actor.did, CALENDAR_EVENT, key, eventRef.cid)
		]);
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'show.save',
			subject: uri,
			subjectKind: 'show',
			summary: `${edit ? 'Updated' : 'Announced'} the show “${input.name}” for ${project.profile.name}`,
			details: { project: project.uri, event: `at://${actor.did}/${CALENDAR_EVENT}/${key}` }
		});

		return { uri, cid };
	}

	/**
	 * Deletes a show `actor` announced, and its calendar event: the event first, since deleting a record that isn't
	 * there does nothing. So it doesn't matter whether HappyView has the event yet, and a delete that fails partway
	 * leaves the show listed, to delete again.
	 */
	async remove(actor: Viewer, project: ProjectView, rkey: string): Promise<void> {
		const uri = `at://${actor.did}/${COLLECTION.show}/${rkey}`;
		const show = await this.indexed.getRecord(actor.did, COLLECTION.show, rkey, { strict: true });
		const listed = show && readShow(show.value);

		if (listed?.project !== project.uri) throw new NotFoundError('That show does not exist');

		await writeOwn(this.core, actor.did, REAUTH, async (client) => {
			await client.deleteRecord({ collection: CALENDAR_EVENT, rkey });
			await client.deleteRecord({ collection: COLLECTION.show, rkey });
		});

		await Promise.all([
			this.awaitServed(actor.did, async () => (await this.served(uri)) === null),
			this.indexed.awaitWrite(actor.did, CALENDAR_EVENT, rkey, null)
		]);
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'show.delete',
			subject: uri,
			subjectKind: 'show',
			summary: `Deleted the show “${listed.name}” for ${project.profile.name}`,
			details: { project: project.uri }
		});
	}

	/**
	 * Has HappyView re-read each manager's shows from their PDS, one account at a time, for any that Jetstream never
	 * brought (while HappyView was down, say). Runs when the app boots.
	 */
	async reindexManagers(): Promise<void> {
		for (const did of await this.projects.managers())
			await reindex(this.directory, this.core.logger, COLLECTION.show, did);
	}

	/**
	 * `did`'s show with key `rkey` and its calendar event, as HappyView has them. Strict, since a write goes on what
	 * this reads, and a HappyView that's down would look like a show that isn't there.
	 */
	private async stored(did: string, rkey: string) {
		const uris = [COLLECTION.show, CALENDAR_EVENT].map((collection) => `at://${did}/${collection}/${rkey}`);
		const records = await this.indexed.getRecords(uris, { strict: true });

		return uris.map((uri) => records.get(uri) ?? null);
	}

	private async served(uri: string): Promise<DirectoryShow | null> {
		return (await this.directory.query('getShows', { uris: [uri] })).shows[0] ?? null;
	}

	/** Waits for the directory to serve a show write by `did`; when it doesn't in time, HappyView re-reads their shows. */
	private async awaitServed(did: string, served: () => Promise<boolean>): Promise<void> {
		if (await awaitIndexed(served, this.indexWaitMs)) return;
		this.core.logger.warn({ did }, 'the directory has not caught up with a show write yet');
		await reindex(this.directory, this.core.logger, COLLECTION.show, did);
	}

	/** The app's view of a directory show, or null (logged) when it or its project doesn't validate. */
	private toView(show: DirectoryShow): ShowView | null {
		const parts = parsePublicRecordUri(show.uri);
		const project = this.projects.toView(show.project);
		const record = readShow(show.record);

		if (parts?.collection !== COLLECTION.show || !project || !record) {
			this.core.logger.warn({ uri: show.uri }, 'skipping an invalid show from the directory');

			return null;
		}

		const view: ShowView = { uri: show.uri, cid: show.cid, did: parts.did, rkey: parts.rkey, record, project };

		if (show.distanceM !== undefined) view.distanceKm = show.distanceM / 1000;

		return view;
	}
}
