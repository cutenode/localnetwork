import { TID } from '@atproto/common-web';
import { sql } from 'kysely';
import { COLLECTION, SPACE_TYPE } from '#lib/nsid.ts';
import { BOOKING_STATUSES, isOneOf } from '#lib/taxonomy/options.ts';
import type { BookingEventView, BookingThreadView, ShowDetails, Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { bookingSpace, parseSpaceRecordUri, parseSpaceUri } from '../atproto/spaces';
import { type Core, ForbiddenError, NotFoundError, UserError } from '../core';
import { nowIso, parseJson } from '../db';
import type { BookingThreadRow } from '../db/schema';
import { isFresh } from '../notifications/notifier';
import { spaceWriter } from './own-repo';
import type { ProjectService } from './projects';
import { actorRole } from './requests';

type EventKind = 'inquiry' | 'message' | 'status';

const KIND_BY_COLLECTION = new Map<string, EventKind>([
	[COLLECTION.bookingInquiry, 'inquiry'],
	[COLLECTION.bookingMessage, 'message'],
	[COLLECTION.bookingStatus, 'status']
]);

/**
 * Every field the booking inquiry, message, and status lexicons define. Each
 * record carries its own collection's subset.
 */
export type BookingRecord = {
	project?: string;
	show?: ShowDetails;
	message?: string;
	text?: string;
	status?: string;
	note?: string;
	onBehalfOf?: string;
	createdAt: string;
};

const OPENED_PROJECT = sql<string>`json_extract(record, '$.project')`;

/**
 * The project each thread's opening record names, or only threads it names `project` for. The root writes that record
 * into its own repo in the space, so unlike the promoter's inquiry, nobody else can rewrite it. Threads opened before
 * it named the project have none.
 */
export const openedProjects = (core: Pick<Core, 'db' | 'rootDid'>, project?: string) =>
	core.db
		.selectFrom('bookingEvent')
		.select(['spaceUri', OPENED_PROJECT.as('project')])
		.where('authorDid', '=', core.rootDid)
		.where('kind', '=', 'status')
		.where(OPENED_PROJECT, project ? '=' : 'is not', project ?? null);

const AUDITED_PROJECT = sql<string>`json_extract(details, '$.project')`;

/**
 * The project each thread's `booking.create` audit entry names, or only threads it names `project` for. The server
 * writes that entry when the thread opens, and a lost one is rebuilt only from the root's chained copy, never from the
 * promoter's own (AuditLog.onSyncedCopy), so it vouches for threads opened before the opening record named a project.
 */
export const auditedProjects = (core: Pick<Core, 'db'>, project?: string) =>
	core.db
		.selectFrom('auditEntry')
		.select(['subject as spaceUri', AUDITED_PROJECT.as('project')])
		.where('action', '=', 'booking.create')
		.where('outcome', '=', 'success')
		.where('subject', 'is not', null)
		.where(AUDITED_PROJECT, project ? '=' : 'is not', project ?? null)
		.$narrowType<{ spaceUri: string }>();

/**
 * Booking threads. Each thread is its own space under the root authority
 * whose member list is the promoter plus the project's managers; each party
 * writes messages into their own repo in that space.
 */
export class BookingService {
	constructor(
		private readonly core: Core,
		private readonly projects: ProjectService
	) {}

	async start(input: { actor: Viewer; projectUri: string; show: ShowDetails; message: string }): Promise<string> {
		const { actor } = input;
		// The root's repo, not the directory, so a manager change that's still being indexed is already in the thread.
		const project = await this.projects.current(input.projectUri);

		if (!project || project.status !== 'active') throw new NotFoundError('That project is not available');

		if (project.managers.length === 0)
			throw new UserError("This project hasn't been claimed yet, so nobody can answer an inquiry");

		if (project.managers.includes(actor.did)) throw new UserError("You can't send an inquiry to your own project");
		const message = input.message.trim();

		if (!message) throw new UserError('Write a message');

		const root = await this.core.root();
		const skey = TID.nextStr();
		const space = bookingSpace(this.core.rootDid, skey);
		await root.createSpace({
			spaceType: SPACE_TYPE.booking,
			skey,
			readPolicy: { $type: 'com.atproto.simplespace.defs#memberListPolicy' },
			writePolicy: { $type: 'com.atproto.simplespace.defs#memberListPolicy' },
			appAccess: { $type: 'com.atproto.simplespace.defs#open' }
		});

		const participants: { did: string; role: 'promoter' | 'manager' }[] = [
			{ did: actor.did, role: 'promoter' },
			...project.managers.map((did) => ({ did, role: 'manager' as const }))
		];

		for (const participant of participants) {
			await root.putMember({ space, did: participant.did, read: true, write: true });
		}

		const now = nowIso();
		await this.core.db.transaction().execute(async (trx) => {
			await trx
				.insertInto('bookingThread')
				.values({
					spaceUri: space,
					skey,
					projectUri: project.uri,
					promoterDid: actor.did,
					status: 'open',
					title: input.show.title,
					showDate: input.show.date ?? null,
					createdAt: now,
					updatedAt: now,
					lastActivityAt: now
				})
				.execute();
			await trx
				.insertInto('bookingParticipant')
				.values(participants.map((p) => ({ spaceUri: space, ...p })))
				.execute();
		});

		const opened = validateRecord(COLLECTION.bookingStatus, {
			status: 'open',
			note: 'Thread opened',
			project: project.uri,
			createdAt: now
		});

		const openedRef = await root.createSpaceRecord({ space, collection: COLLECTION.bookingStatus, record: opened });
		await this.indexEvent({
			space,
			uri: openedRef.uri,
			repoDid: root.did,
			collection: COLLECTION.bookingStatus,
			value: opened
		});

		const { client, onBehalf } = await spaceWriter(this.core, actor);

		const draft: BookingRecord = {
			project: project.uri,
			show: JSON.parse(JSON.stringify(input.show)),
			message,
			createdAt: nowIso()
		};

		if (onBehalf) draft.onBehalfOf = actor.did;
		const inquiry = validateRecord(COLLECTION.bookingInquiry, draft);

		const ref = await client.createSpaceRecord({ space, collection: COLLECTION.bookingInquiry, record: inquiry });
		await this.indexEvent({
			space,
			uri: ref.uri,
			repoDid: client.did,
			collection: COLLECTION.bookingInquiry,
			value: inquiry
		});
		await this.core.watchSpace?.(space);

		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'booking.create',
				subject: space,
				subjectKind: 'booking',
				summary: `Sent a booking inquiry to ${project.profile.name}`,
				details: { project: project.uri, inquiry: ref.uri, showDate: input.show.date, onBehalf }
			},
			{ actorClient: onBehalf ? null : client }
		);

		return space;
	}

	private async requireParticipant(actor: Viewer, space: string): Promise<BookingThreadRow> {
		const thread = await this.core.db
			.selectFrom('bookingThread')
			.selectAll()
			.where('spaceUri', '=', space)
			.executeTakeFirst();

		if (!thread) throw new NotFoundError('Booking thread not found');

		const participant = await this.core.db
			.selectFrom('bookingParticipant')
			.select('role')
			.where('spaceUri', '=', space)
			.where('did', '=', actor.did)
			.executeTakeFirst();

		if (!participant) throw new ForbiddenError('You are not part of this booking thread');

		return thread;
	}

	async message(input: { actor: Viewer; space: string; text: string }): Promise<void> {
		const text = input.text.trim();

		if (!text) throw new UserError('Write a message');
		await this.requireParticipant(input.actor, input.space);
		const { client, onBehalf } = await spaceWriter(this.core, input.actor);

		const draft: BookingRecord = { text, createdAt: nowIso() };

		if (onBehalf) draft.onBehalfOf = input.actor.did;
		const record = validateRecord(COLLECTION.bookingMessage, draft);

		const ref = await client.createSpaceRecord({ space: input.space, collection: COLLECTION.bookingMessage, record });
		await this.indexEvent({
			space: input.space,
			uri: ref.uri,
			repoDid: client.did,
			collection: COLLECTION.bookingMessage,
			value: record
		});
		await this.core.audit.record(
			{
				actor: input.actor.did,
				actorRole: actorRole(input.actor),
				action: 'booking.message',
				subject: input.space,
				subjectKind: 'booking',
				summary: 'Sent a booking message',
				details: { message: ref.uri, length: text.length, onBehalf }
			},
			{ actorClient: onBehalf ? null : client }
		);
	}

	async setStatus(input: { actor: Viewer; space: string; status: string; note?: string }): Promise<void> {
		if (!isOneOf(BOOKING_STATUSES, input.status)) throw new UserError('Unknown status');
		const thread = await this.requireParticipant(input.actor, input.space);

		if (thread.status === input.status) return;
		const { client, onBehalf } = await spaceWriter(this.core, input.actor);

		const draft: BookingRecord = { status: input.status, createdAt: nowIso() };
		const note = input.note?.trim();

		if (note) draft.note = note;

		if (onBehalf) draft.onBehalfOf = input.actor.did;
		const record = validateRecord(COLLECTION.bookingStatus, draft);

		const ref = await client.createSpaceRecord({ space: input.space, collection: COLLECTION.bookingStatus, record });
		await this.indexEvent({
			space: input.space,
			uri: ref.uri,
			repoDid: client.did,
			collection: COLLECTION.bookingStatus,
			value: record
		});
		await this.core.audit.record(
			{
				actor: input.actor.did,
				actorRole: actorRole(input.actor),
				action: 'booking.status',
				subject: input.space,
				subjectKind: 'booking',
				summary: `Marked a booking ${input.status}`,
				changes: [{ field: 'status', before: thread.status, after: input.status }],
				details: { status: ref.uri, onBehalf }
			},
			{ actorClient: onBehalf ? null : client }
		);
	}

	/** Restores the participant list from the space's member list (after a fresh sync). */
	private async ensureParticipants(space: string): Promise<Set<string>> {
		const rows = await this.core.db
			.selectFrom('bookingParticipant')
			.select('did')
			.where('spaceUri', '=', space)
			.execute();

		if (rows.length > 0) return new Set(rows.map((row) => row.did));

		try {
			const members = await (await this.core.root()).listMembers(space);

			return new Set(members.map((member) => member.did));
		} catch {
			return new Set();
		}
	}

	/** Forgets a thread whose space is gone. Its participants go with it (ON DELETE CASCADE). */
	async forget(space: string): Promise<void> {
		await this.core.db.deleteFrom('bookingEvent').where('spaceUri', '=', space).execute();
		await this.core.db.deleteFrom('bookingThread').where('spaceUri', '=', space).execute();
	}

	/** Indexes a booking record from our own write or from sync, ignoring records by non-participants. */
	async indexEvent(input: {
		space: string;
		uri: string;
		repoDid: string;
		collection: string;
		value: BookingRecord;
	}): Promise<void> {
		const kind = KIND_BY_COLLECTION.get(input.collection);
		const ref = parseSpaceUri(input.space);

		if (!kind || !ref || !parseSpaceRecordUri(input.uri)) return;
		const { value } = input;

		const fromRoot = input.repoDid === this.core.rootDid;
		const author = fromRoot && value.onBehalfOf ? value.onBehalfOf : input.repoDid;
		const participants = await this.ensureParticipants(input.space);

		if (!fromRoot && !participants.has(input.repoDid)) {
			this.core.logger.warn(
				{ space: input.space, repo: input.repoDid },
				'ignoring booking record from non-participant'
			);

			return;
		}

		const { createdAt } = value;
		const { db } = this.core;

		let thread = await db
			.selectFrom('bookingThread')
			.selectAll()
			.where('spaceUri', '=', input.space)
			.executeTakeFirst();

		if (!thread && kind === 'inquiry' && value.project) {
			// The promoter can rewrite their inquiry, so the root's opening record names the project when it's in.
			const opened = await openedProjects(this.core).where('spaceUri', '=', input.space).executeTakeFirst();
			const projectUri = opened?.project ?? value.project;
			await db
				.insertInto('bookingThread')
				.values({
					spaceUri: input.space,
					skey: ref.skey,
					projectUri,
					promoterDid: author,
					status: 'open',
					title: value.show?.title ?? null,
					showDate: value.show?.date ?? null,
					createdAt,
					updatedAt: createdAt,
					lastActivityAt: createdAt
				})
				.onConflict((oc) => oc.doNothing())
				.execute();
			const project = await this.projects.byUri(projectUri);

			const members = [
				{ did: author, role: 'promoter' as const },
				...(project?.managers ?? [])
					.filter((did) => participants.has(did))
					.map((did) => ({ did, role: 'manager' as const }))
			];

			await db
				.insertInto('bookingParticipant')
				.values(members.map((m) => ({ spaceUri: input.space, ...m })))
				.onConflict((oc) => oc.doNothing())
				.execute();
			thread = await db.selectFrom('bookingThread').selectAll().where('spaceUri', '=', input.space).executeTakeFirst();
		}

		await db
			.insertInto('bookingEvent')
			.values({
				uri: input.uri,
				spaceUri: input.space,
				authorDid: author,
				kind,
				record: JSON.stringify(input.value),
				createdAt
			})
			.onConflict((oc) => oc.column('uri').doUpdateSet({ record: JSON.stringify(input.value) }))
			.execute();

		if (!thread) return;
		const latest = createdAt > thread.lastActivityAt ? createdAt : thread.lastActivityAt;
		const update: Partial<BookingThreadRow> = { lastActivityAt: latest, updatedAt: nowIso() };

		if (kind === 'status' && value.status && createdAt >= thread.lastActivityAt) update.status = value.status;
		// The root's own record opening the thread, which the inquiry right after it tells people about.
		const opening = kind === 'status' && author === this.core.rootDid ? value.project : undefined;

		// Whatever project the inquiry named, the root's opening record decides it.
		if (opening) update.projectUri = opening;
		await db.updateTable('bookingThread').set(update).where('spaceUri', '=', input.space).execute();

		if (!opening) await this.notifyParticipants({ kind, uri: input.uri, author, thread, value });
	}

	/**
	 * Tells the thread's other participants about a record in it, once each, however often it's indexed. A failure is
	 * only logged: the write or sync that indexed the record stands, and the thread page shows it either way.
	 */
	private async notifyParticipants(input: {
		kind: EventKind;
		uri: string;
		author: string;
		thread: BookingThreadRow;
		value: BookingRecord;
	}): Promise<void> {
		const { kind, author, thread, value } = input;

		// An old record is one a sync replays, so it saves the directory lookup below.
		if (!isFresh(value.createdAt)) return;

		try {
			const participants = await this.core.db
				.selectFrom('bookingParticipant')
				.select('did')
				.where('spaceUri', '=', thread.spaceUri)
				.where('did', '!=', author)
				.execute();

			if (participants.length === 0) return;
			const project = await this.projects.byUri(thread.projectUri).catch(() => null);

			// The act speaks as itself: which of its people wrote stays out (docs/adr/0004).
			const data = {
				skey: thread.skey,
				title: thread.title,
				projectName: project?.profile.name ?? null,
				from: author === thread.promoterDid ? ('promoter' as const) : ('act' as const)
			};

			const notice = {
				to: participants.map((participant) => participant.did),
				key: input.uri,
				subject: thread.spaceUri,
				href: `/dashboard/bookings/${thread.skey}`,
				at: value.createdAt
			};

			if (kind === 'status') {
				if (value.status)
					await this.core.notify({ ...notice, kind: 'booking.status', data: { ...data, status: value.status } });
			} else {
				await this.core.notify({ ...notice, kind: kind === 'inquiry' ? 'booking.inquiry' : 'booking.message', data });
			}
		} catch (error) {
			this.core.logger.warn(
				{ err: error, space: thread.spaceUri, uri: input.uri },
				'could not notify a booking thread'
			);
		}
	}

	async threadsFor(did: string): Promise<BookingThreadView[]> {
		const rows = await this.core.db
			.selectFrom('bookingThread')
			.innerJoin('bookingParticipant', 'bookingParticipant.spaceUri', 'bookingThread.spaceUri')
			.selectAll('bookingThread')
			.where('bookingParticipant.did', '=', did)
			.orderBy('bookingThread.lastActivityAt', 'desc')
			.execute();

		return this.toViews(rows);
	}

	async thread(viewer: Viewer, space: string): Promise<{ thread: BookingThreadView; events: BookingEventView[] }> {
		const row = await this.requireParticipant(viewer, space);
		const [thread] = await this.toViews([row]);

		const events = await this.core.db
			.selectFrom('bookingEvent')
			.selectAll()
			.where('spaceUri', '=', space)
			.orderBy('createdAt')
			.execute();

		const accounts = await this.core.accounts.views(events.map((event) => event.authorDid));

		return {
			thread,
			events: events
				.filter((event) => event.authorDid !== this.core.rootDid)
				.map((event) => {
					const record = parseJson<Partial<BookingRecord>>(event.record, {});

					return {
						uri: event.uri,
						kind: event.kind,
						author: accounts.get(event.authorDid) ?? { did: event.authorDid, handle: null },
						createdAt: event.createdAt,
						text: record.text ?? record.message ?? record.note,
						status: record.status,
						show: record.show
					};
				})
		};
	}

	private async toViews(rows: BookingThreadRow[]): Promise<BookingThreadView[]> {
		if (rows.length === 0) return [];
		const spaces = rows.map((row) => row.spaceUri);

		const participants = await this.core.db
			.selectFrom('bookingParticipant')
			.selectAll()
			.where('spaceUri', 'in', spaces)
			.execute();

		const accounts = await this.core.accounts.views(participants.map((p) => p.did));
		const projects = await this.projects.byUris(rows.map((row) => row.projectUri));

		return rows.map((row) => {
			const project = projects.get(row.projectUri);

			return {
				space: row.spaceUri,
				skey: row.skey,
				status: row.status,
				title: row.title,
				showDate: row.showDate,
				project: project ? { uri: project.uri, slug: project.slug, name: project.profile.name } : null,
				promoter: accounts.get(row.promoterDid) ?? { did: row.promoterDid, handle: null },
				participants: participants
					.filter((p) => p.spaceUri === row.spaceUri)
					.map((p) => ({ ...(accounts.get(p.did) ?? { did: p.did, handle: null }), role: p.role })),
				lastActivityAt: row.lastActivityAt,
				createdAt: row.createdAt
			};
		});
	}

	async bySkey(skey: string): Promise<string | null> {
		const row = await this.core.db
			.selectFrom('bookingThread')
			.select('spaceUri')
			.where('skey', '=', skey)
			.executeTakeFirst();

		return row?.spaceUri ?? null;
	}
}
