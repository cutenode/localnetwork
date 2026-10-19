import { TID } from '@atproto/common-web';
import type { JsonObject } from '@atproto/lex-json';
import { z } from 'zod';
import { type LatLng, toE6 } from '#lib/geo.ts';
import { CALENDAR_EVENT, COLLECTION } from '#lib/nsid.ts';
import { EVENT_STATUS, type ShowFormValues, venueClock, venueDay } from '#lib/shows.ts';
import type { Location, ProjectView, ShowRecord, ShowView, StrongRef, Viewer } from '#lib/types.ts';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import type { PublicRepo } from '../atproto/public-repo';
import { recordCid, type RepoClient, type RepoWrite } from '../atproto/repo-client';
import { parsePublicRecordUri } from '../atproto/spaces';
import { type Core, ForbiddenError, NotFoundError, UserError } from '../core';
import { nowIso } from '../db';
import type { DirectoryShow, HappyViewClient } from '../happyview/client';
import { awaitIndexed, reindex } from '../happyview/indexed';
import { atVenue, venueTime } from '../venue-time';
import { writeOwn } from './own-repo';
import { INDEX_WAIT_MS, type ProjectService } from './projects';
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
function formValues(show: ShowRecord, event: EventRecord | null): ShowFormValues {
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
 * copies what listings need. HappyView indexes the shows (lua/computer/localnetwork/show.lua) and answers every
 * listing; the events stay in the managers' repos.
 */
export class ShowService {
	constructor(
		private readonly core: Core,
		private readonly projects: ProjectService,
		private readonly directory: HappyViewClient,
		private readonly repos: PublicRepo,
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
	 * A show `actor` announced for `project`, as the announce form's values, read from their repo: the calendar
	 * event as it is now, since another app may have changed it. Null when there's no such show.
	 */
	async values(actor: Viewer, project: ProjectView, rkey: string): Promise<ShowFormValues | null> {
		const [show, event] = await Promise.all([
			this.repos.getRecord(actor.did, COLLECTION.show, rkey),
			this.repos.getRecord(actor.did, CALENDAR_EVENT, rkey)
		]);

		const record = show && readShow(show.value);

		if (record?.project !== project.uri) return null;

		return formValues(record, event && readEvent(event.value));
	}

	/**
	 * Of `did`'s shows in `shows`, the ones whose calendar event changed after the show copied it, read from their
	 * repo. A PDS that doesn't answer reads as no change, since a missing event and an unreachable one look the same.
	 */
	async edited(did: string, shows: ShowView[]): Promise<string[]> {
		const own = shows.filter((show) => show.did === did);

		const events = await Promise.all(
			own.map(async (show) => {
				const parts = parsePublicRecordUri(show.record.event.uri);

				return parts?.did === did ? this.repos.getRecord(did, parts.collection, parts.rkey) : null;
			})
		);

		return own.flatMap((show, i) => (events[i] && events[i].cid !== show.record.event.cid ? [show.uri] : []));
	}

	/**
	 * Announces a show, or updates one `actor` announced: the calendar event and the show copying it, in one write, so
	 * neither is ever left without the other. The event's CID is worked out here, the way the PDS works it out.
	 */
	async save(actor: Viewer, project: ProjectView, input: ShowInput, rkey?: string): Promise<StrongRef> {
		if (!project.managers.includes(actor.did))
			throw new ForbiddenError("Only this project's managers can announce its shows");

		if (project.status !== 'active') throw new UserError('Only listed projects can announce shows');
		const key = rkey ?? TID.nextStr();
		const uri = `at://${actor.did}/${COLLECTION.show}/${key}`;

		const cid = await writeOwn(this.core, actor.did, REAUTH, async (client) => {
			const [show, event] = rkey ? await this.stored(client, rkey) : [null, null];
			const listed = show && readShow(show.value);

			if (rkey && listed?.project !== project.uri) throw new NotFoundError('That show does not exist');
			const written = validateRecord(CALENDAR_EVENT, eventRecord(input, event?.value, listed));
			const eventRef = { uri: `at://${actor.did}/${CALENDAR_EVENT}/${key}`, cid: await recordCid(written) };
			const copy = showRecord(project.uri, eventRef, input, written, listed?.createdAt);
			const record = validateRecord(COLLECTION.show, JSON.parse(JSON.stringify(copy)));

			await client.applyWrites([
				{ action: event ? 'update' : 'create', collection: CALENDAR_EVENT, rkey: key, record: written },
				{ action: show ? 'update' : 'create', collection: COLLECTION.show, rkey: key, record }
			]);

			return recordCid(record);
		});

		await this.awaitServed(actor.did, async () => (await this.served(uri))?.cid === cid);
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'show.save',
			subject: uri,
			subjectKind: 'show',
			summary: `${rkey ? 'Updated' : 'Announced'} the show “${input.name}” for ${project.profile.name}`,
			details: { project: project.uri, event: `at://${actor.did}/${CALENDAR_EVENT}/${key}` }
		});

		return { uri, cid };
	}

	/** Deletes a show `actor` announced, and its calendar event, in one write. */
	async remove(actor: Viewer, project: ProjectView, rkey: string): Promise<void> {
		const uri = `at://${actor.did}/${COLLECTION.show}/${rkey}`;

		const name = await writeOwn(this.core, actor.did, REAUTH, async (client) => {
			const [show, event] = await this.stored(client, rkey);
			const listed = show && readShow(show.value);

			if (listed?.project !== project.uri) throw new NotFoundError('That show does not exist');
			const writes: RepoWrite[] = [{ action: 'delete', collection: COLLECTION.show, rkey }];

			if (event) writes.push({ action: 'delete', collection: CALENDAR_EVENT, rkey });
			await client.applyWrites(writes);

			return listed.name;
		});

		await this.awaitServed(actor.did, async () => (await this.served(uri)) === null);
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'show.delete',
			subject: uri,
			subjectKind: 'show',
			summary: `Deleted the show “${name}” for ${project.profile.name}`,
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

	/** The show with key `rkey` in the client's own repo, and its calendar event. */
	private stored(client: RepoClient, rkey: string) {
		return Promise.all([
			client.getRecord({ collection: COLLECTION.show, rkey }),
			client.getRecord({ collection: CALENDAR_EVENT, rkey })
		]);
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
