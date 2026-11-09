import { XRPCError } from '@atproto/api';
import { TID } from '@atproto/common-web';
import { sql } from 'kysely';
import { bookingActions, type BookingRole } from '#lib/bookings.ts';
import { COLLECTION, SPACE_TYPE } from '#lib/nsid.ts';
import { BOOKING_STATUSES, isOneOf, labelFor } from '#lib/taxonomy/options.ts';
import type { BookingEventView, BookingThreadView, ShowDetails, Viewer } from '#lib/types.ts';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import { type JsonRecord, memberSpacePolicy, type RecordRef } from '../atproto/repo-client';
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
	promoter?: string;
	onBehalfOf?: string;
	createdAt: string;
};

/**
 * When a record dated `createdAt` counts as written. Whoever writes a record picks its date, so a date that hasn't come
 * yet counts as now, and can't keep a thread at the top of everyone's list or leave it waiting on a reply it has.
 */
function seenAt(createdAt: string): string {
	const now = nowIso();

	return createdAt < now ? createdAt : now;
}

/** A thread's last activity once a record dated `createdAt` joins it. */
function activityAfter(lastActivityAt: string, createdAt: string): string {
	const at = seenAt(createdAt);

	return at > lastActivityAt ? at : lastActivityAt;
}

/** What a status record read from a repo says opened its thread, or null when it doesn't fit its lexicon. */
function readStatus(value: JsonRecord): Pick<BookingRecord, 'project' | 'promoter'> | null {
	try {
		// SAFETY: validateRecord checked `value` against the booking status lexicon, whose `project` is an at-uri string
		// and `promoter` a DID string, both optional.
		return validateRecord(COLLECTION.bookingStatus, value) as Pick<BookingRecord, 'project' | 'promoter'>;
	} catch (error) {
		if (error instanceof RecordValidationError) return null;
		throw error;
	}
}

const OPENED_PROJECT = sql<string>`json_extract(record, '$.project')`;

const OPENED_BY = sql<string | null>`json_extract(record, '$.promoter')`;

/**
 * The project and promoter each thread's opening record names, or only threads it names `project` for. The root writes
 * that record into its own repo in the space, so unlike the promoter's inquiry, nobody else can rewrite it. Threads
 * opened before it named the project have none, and those opened before it named the promoter have no promoter.
 */
export const openedProjects = (core: Pick<Core, 'db' | 'rootDid'>, project?: string) =>
	core.db
		.selectFrom('bookingEvent')
		.select(['spaceUri', OPENED_PROJECT.as('project'), OPENED_BY.as('promoter')])
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

		// The inquiry is checked before anything is written, so one that doesn't fit its record opens no thread.
		const now = nowIso();
		const { client, onBehalf } = await spaceWriter(this.core, actor);

		const draft: BookingRecord = {
			project: project.uri,
			show: JSON.parse(JSON.stringify(input.show)),
			message,
			createdAt: nowIso()
		};

		if (onBehalf) draft.onBehalfOf = actor.did;
		const inquiry = validateRecord(COLLECTION.bookingInquiry, draft);

		const root = await this.core.root();
		const skey = TID.nextStr();
		const space = bookingSpace(this.core.rootDid, skey);

		try {
			await root.createSpace({
				spaceType: SPACE_TYPE.booking,
				skey,
				policy: memberSpacePolicy()
			});
		} catch (error) {
			if (error instanceof XRPCError) throw this.unsent(error);
			throw error;
		}

		const participants: { did: string; role: 'promoter' | 'manager' }[] = [
			{ did: actor.did, role: 'promoter' },
			...project.managers.map((did) => ({ did, role: 'manager' as const }))
		];

		let ref: RecordRef;

		try {
			for (const participant of participants) {
				await root.putMember({ space, did: participant.did, read: true, write: true });
			}

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
				promoter: actor.did,
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

			ref = await client.createSpaceRecord({ space, collection: COLLECTION.bookingInquiry, record: inquiry });
		} catch (error) {
			// The inquiry is what makes it a thread: without it, nobody should see one. Past it, a failure leaves a real
			// thread, which a sync would index anyway.
			await this.forget(space);
			await root.deleteSpace(space).catch((cause: unknown) => {
				this.core.logger.warn('could not delete the space of a booking that failed to start', { err: cause, space });
			});

			if (error instanceof XRPCError) throw this.unsent(error);
			throw error;
		}

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

	/**
	 * A PDS that refused or never answered a write while a booking started, as a message the inquiry form shows above
	 * what the promoter wrote, so they can send it again. Any other error is a bug, so callers rethrow it as it is.
	 */
	private unsent(error: XRPCError): UserError {
		this.core.logger.warn('could not start a booking thread', { err: error });

		return new UserError("Couldn't send your inquiry. Try again in a minute.", 503);
	}

	private async requireParticipant(actor: Viewer, space: string): Promise<BookingThreadRow & { role: BookingRole }> {
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

		return { ...thread, role: participant.role };
	}

	/** Whether any of the thread's managers has written in it yet. */
	private async answered(space: string): Promise<boolean> {
		const reply = await this.core.db
			.selectFrom('bookingEvent')
			.innerJoin('bookingParticipant', (join) =>
				join
					.onRef('bookingParticipant.spaceUri', '=', 'bookingEvent.spaceUri')
					.onRef('bookingParticipant.did', '=', 'bookingEvent.authorDid')
			)
			.select('bookingEvent.uri')
			.where('bookingEvent.spaceUri', '=', space)
			.where('bookingEvent.kind', '!=', 'unapplied')
			.where('bookingParticipant.role', '=', 'manager')
			.executeTakeFirst();

		return reply !== undefined;
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
		const allowed = bookingActions(thread.role, thread.status, await this.answered(input.space));

		// Each side has its own moves (bookingActions); anything else, including the status it already has, is refused.
		if (!allowed.some((action) => action.status === input.status)) throw await this.refusal(input, thread);
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

		// The other side can move the thread between the check above and this write, and then this one didn't count.
		const indexed = await this.core.db
			.selectFrom('bookingEvent')
			.select('kind')
			.where('uri', '=', ref.uri)
			.executeTakeFirstOrThrow();

		if (indexed.kind === 'unapplied')
			throw await this.refusal(input, await this.requireParticipant(input.actor, input.space));

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

	/** Logs a status change `thread` doesn't allow, and returns the error that tells whoever tried. */
	private async refusal(
		input: { actor: Viewer; space: string; status: string },
		thread: BookingThreadRow & { role: BookingRole }
	): Promise<UserError> {
		const [from, to] = [thread.status, input.status].map((status) => labelFor(BOOKING_STATUSES, status).toLowerCase());

		const message =
			from === to ? `This booking is already ${to}` : `You can't mark this booking ${to} while it's ${from}`;

		await this.core.audit.record({
			actor: input.actor.did,
			actorRole: actorRole(input.actor),
			action: 'booking.status',
			subject: input.space,
			subjectKind: 'booking',
			summary: `Denied: ${message}`,
			outcome: 'denied',
			reason: message,
			details: { role: thread.role, from: thread.status, to: input.status }
		});

		return new UserError(message, 409);
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

	/**
	 * What the root's record opening the thread in `space` names, from the index, or else from the root's own repo,
	 * since a sync can read an inquiry before it. Undefined for a thread with no opening record, or none that can be
	 * read right now.
	 */
	private async openingRecord(space: string): Promise<{ project: string; promoter: string | null } | undefined> {
		const indexed = await openedProjects(this.core).where('spaceUri', '=', space).executeTakeFirst();

		if (indexed) return indexed;
		const root = await this.core.root();
		let cursor: string | undefined;

		try {
			do {
				const page = await root.listSpaceRecords({
					space,
					repo: root.did,
					collection: COLLECTION.bookingStatus,
					cursor
				});

				const opening = page.records.map((record) => readStatus(record.value)).find((status) => status?.project);

				if (opening?.project) return { project: opening.project, promoter: opening.promoter ?? null };
				cursor = page.cursor;
			} while (cursor);
		} catch (error) {
			this.core.logger.warn("could not read a booking thread's opening record", { err: error, space });
		}

		return undefined;
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
			this.core.logger.warn('ignoring booking record from non-participant', {
				space: input.space,
				repo: input.repoDid
			});

			return;
		}

		const createdAt = seenAt(value.createdAt);
		const { db } = this.core;

		let thread = await db
			.selectFrom('bookingThread')
			.selectAll()
			.where('spaceUri', '=', input.space)
			.executeTakeFirst();

		if (!thread && kind === 'inquiry' && value.project) {
			// The promoter can rewrite their inquiry, so the root's opening record names the project, and who the promoter
			// is, over whichever inquiry a sync reads first.
			const opened = await this.openingRecord(input.space);
			const projectUri = opened?.project ?? value.project;
			const project = await this.projects.byUri(projectUri);

			// Threads opened before the record named the promoter go by who sent it: nobody can send an inquiry to their
			// own project (start), so one from the act's side never opens a thread.
			const promoter = opened?.promoter ? opened.promoter === author : !project?.managers.includes(author);

			if (!promoter) {
				this.core.logger.warn("not opening a booking thread from an inquiry its promoter didn't send", {
					space: input.space,
					repo: input.repoDid
				});
			} else {
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
				thread = await db
					.selectFrom('bookingThread')
					.selectAll()
					.where('spaceUri', '=', input.space)
					.executeTakeFirst();
			}
		}

		// A status from either side waits here until applyStatuses finds the thread's rules allow it. The root's own status
		// records only open threads, and one that names no status moves nothing.
		const isMove = kind === 'status' && !!value.status && author !== this.core.rootDid;

		await db
			.insertInto('bookingEvent')
			.values({
				uri: input.uri,
				spaceUri: input.space,
				authorDid: author,
				kind: isMove ? 'unapplied' : kind,
				record: JSON.stringify(input.value),
				createdAt
			})
			// A status record stands as first read, so rewriting one later can't change what it did.
			.onConflict((oc) =>
				kind === 'status'
					? oc.column('uri').doNothing()
					: oc.column('uri').doUpdateSet({ record: JSON.stringify(input.value) })
			)
			.execute();

		if (!thread) return;

		if (!isMove) {
			const update: Partial<BookingThreadRow> = {
				lastActivityAt: activityAfter(thread.lastActivityAt, createdAt),
				updatedAt: nowIso()
			};

			// The root's own record opening the thread, which the inquiry right after it tells people about.
			const opening = kind === 'status' && author === this.core.rootDid ? value.project : undefined;

			// Whatever project the inquiry named, the root's opening record decides it.
			if (opening) update.projectUri = opening;
			await db.updateTable('bookingThread').set(update).where('spaceUri', '=', input.space).execute();

			if (!opening) await this.notifyParticipants({ kind, uri: input.uri, author, thread, value });
		}

		await this.applyStatuses(input.space);
	}

	/**
	 * Moves a thread through the status records its rules allow, oldest first. Any record can unlock one that got here
	 * before it: a sync reads repos in no particular order, so a confirmation can arrive ahead of the reply that allows it.
	 * The rest wait, and a record that never fits changes nothing and tells nobody. Nothing here goes by a record's date
	 * except which of two waiting records goes first, since its author picks that date (docs/adr/0008).
	 */
	private async applyStatuses(space: string): Promise<void> {
		const { db } = this.core;

		const waiting = await db
			.selectFrom('bookingEvent')
			.selectAll()
			.where('spaceUri', '=', space)
			.where('kind', '=', 'unapplied')
			.orderBy('createdAt')
			.orderBy('uri')
			.execute();

		if (waiting.length === 0) return;

		const participants = await db
			.selectFrom('bookingParticipant')
			.select(['did', 'role'])
			.where('spaceUri', '=', space)
			.execute();

		const roles = new Map(participants.map((participant) => [participant.did, participant.role]));
		const answered = await this.answered(space);

		const moves = waiting.map((event) => ({
			...event,
			value: parseJson<BookingRecord>(event.record, { createdAt: event.createdAt })
		}));

		const thread = await db
			.selectFrom('bookingThread')
			.selectAll()
			.where('spaceUri', '=', space)
			.executeTakeFirstOrThrow();

		let { status: from, lastActivityAt } = thread;

		// Statuses only ever move forward, and each record applies once, so this ends.
		for (;;) {
			const move = moves.find(({ authorDid, value }) => {
				const role = roles.get(authorDid);

				return role && bookingActions(role, from, answered).some((action) => action.status === value.status);
			});

			if (!move?.value.status) {
				if (moves.length > 0)
					this.core.logger.warn('booking status records the thread does not allow', { space, moves: moves.length });

				return;
			}

			const { status } = move.value;
			const activity = activityAfter(lastActivityAt, move.createdAt);

			const applied = await db.transaction().execute(async (trx) => {
				const { numUpdatedRows } = await trx
					.updateTable('bookingThread')
					.set({ status, lastActivityAt: activity, updatedAt: nowIso() })
					.where('spaceUri', '=', space)
					.where('status', '=', from)
					.executeTakeFirstOrThrow();

				if (numUpdatedRows === 0n) return false;
				await trx.updateTable('bookingEvent').set({ kind: 'status' }).where('uri', '=', move.uri).execute();

				return true;
			});

			// Another write moved the thread first, and it applies what's left.
			if (!applied) return;
			await this.notifyParticipants({
				kind: 'status',
				uri: move.uri,
				author: move.authorDid,
				thread,
				value: move.value
			});
			from = status;
			lastActivityAt = activity;
			moves.splice(moves.indexOf(move), 1);
		}
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
		const at = seenAt(value.createdAt);

		// An old record is one a sync replays, so it saves the directory lookup below.
		if (!isFresh(at)) return;

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
				at
			};

			if (kind === 'status') {
				if (value.status)
					await this.core.notify({ ...notice, kind: 'booking.status', data: { ...data, status: value.status } });
			} else {
				await this.core.notify({ ...notice, kind: kind === 'inquiry' ? 'booking.inquiry' : 'booking.message', data });
			}
		} catch (error) {
			this.core.logger.warn('could not notify a booking thread', {
				err: error,
				space: thread.spaceUri,
				uri: input.uri
			});
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
			.where('kind', '!=', 'unapplied')
			.$narrowType<{ kind: BookingEventView['kind'] }>()
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
						author: accounts.get(event.authorDid),
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

		// Newest first, so the first one seen for each thread is its last author. The root only opens threads.
		const latest = await this.core.db
			.selectFrom('bookingEvent')
			.select(['spaceUri', 'authorDid'])
			.where('spaceUri', 'in', spaces)
			.where('authorDid', '!=', this.core.rootDid)
			.where('kind', '!=', 'unapplied')
			.orderBy('createdAt', 'desc')
			.execute();

		const lastAuthors = new Map<string, string>();

		for (const event of latest) if (!lastAuthors.has(event.spaceUri)) lastAuthors.set(event.spaceUri, event.authorDid);

		return rows.map((row) => {
			const project = projects.get(row.projectUri);

			return {
				space: row.spaceUri,
				skey: row.skey,
				status: row.status,
				title: row.title,
				showDate: row.showDate,
				project: project ? { uri: project.uri, slug: project.slug, name: project.profile.name } : null,
				promoter: accounts.get(row.promoterDid),
				participants: participants
					.filter((p) => p.spaceUri === row.spaceUri)
					.map((p) => ({ ...accounts.get(p.did), role: p.role })),
				lastAuthor: lastAuthors.get(row.spaceUri) ?? null,
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
