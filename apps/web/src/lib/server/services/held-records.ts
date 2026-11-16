import { sql } from 'kysely';
import { requestTitle } from '#lib/format.ts';
import { COLLECTION, SPACE_TYPE } from '#lib/nsid.ts';
import type { HeldRecordKind, HeldRecordView, Viewer } from '#lib/types.ts';
import { parseSpaceRecordUri, parseSpaceUri, publicRecordUri, spaceOfRecord } from '../atproto/spaces';
import type { SubjectKind } from '../audit/log';
import { type Core, NotFoundError, UserError } from '../core';
import { nowIso, parseJson } from '../db';
import type { RateLimiter } from '../rate-limit';
import type { BookingRecord } from './bookings';
import type { ProjectService } from './projects';
import { actorRole, type RequestService } from './requests';

/**
 * Where each kind is indexed, and the column naming who the root wrote it for. A kind that `keeps` a decided record's
 * row after deleting it marks the row with `deletedAt`.
 */
const KINDS = {
	request: { table: 'request', person: 'requesterDid', subject: 'request', keeps: true },
	booking: { table: 'bookingEvent', person: 'authorDid', subject: 'booking', keeps: false },
	epkRequest: { table: 'epkRequest', person: 'requesterDid', subject: 'epk', keeps: true },
	epkDraft: { table: 'epkDraft', person: 'authorDid', subject: 'epk', keeps: false }
} as const satisfies Record<HeldRecordKind, { table: string; person: string; subject: SubjectKind; keeps: boolean }>;

export const isHeldRecordKind = (value: string): value is HeldRecordKind => Object.hasOwn(KINDS, value);

/**
 * What each collection is, in an audit summary, and the kind it's listed under. Booking status changes aren't here:
 * the thread's status comes from them, so deleting a cancellation would quietly reopen the booking for the other side.
 */
const COLLECTIONS = new Map<string, { kind: HeldRecordKind; label: string }>([
	[COLLECTION.request, { kind: 'request', label: 'a request to staff' }],
	[COLLECTION.bookingInquiry, { kind: 'booking', label: 'a booking inquiry' }],
	[COLLECTION.bookingMessage, { kind: 'booking', label: 'a booking message' }],
	[COLLECTION.epkRequest, { kind: 'epkRequest', label: 'a press kit access request' }],
	[COLLECTION.epkDraft, { kind: 'epkDraft', label: 'a press kit draft' }]
]);

/**
 * The records the root wrote into spaces on someone's behalf, naming them in `onBehalfOf`. They sit in the root's repo,
 * out of that person's reach, so this lets them see and delete them. Deleting one takes it out of the index too: an
 * open request leaves the queue, as if withdrawn, a decided one keeps its row without what it said, so the decision
 * stands, and a booking message leaves its thread for everyone.
 */
export class HeldRecords {
	constructor(
		private readonly core: Core,
		private readonly requests: RequestService,
		private readonly projects: ProjectService,
		private readonly limiter: RateLimiter
	) {}

	/** Everything the root holds for `viewer`, newest first within each kind. */
	async list(viewer: Viewer): Promise<HeldRecordView[]> {
		const { db, rootDid } = this.core;
		const byRoot = (row: { uri: string }) => parseSpaceRecordUri(row.uri)?.author === rootDid;

		const [requests, events, epkRequests, drafts] = await Promise.all([
			this.requests.list({ requester: viewer.did, limit: 500 }),
			db
				.selectFrom('bookingEvent')
				.leftJoin('bookingThread', 'bookingThread.spaceUri', 'bookingEvent.spaceUri')
				.select(['bookingEvent.uri', 'bookingEvent.kind', 'bookingEvent.record', 'bookingEvent.createdAt'])
				.select('bookingThread.title')
				.where('bookingEvent.authorDid', '=', viewer.did)
				.where('bookingEvent.kind', 'in', ['inquiry', 'message'])
				.orderBy('bookingEvent.createdAt', 'desc')
				.execute(),
			db
				.selectFrom('epkRequest')
				.select(['uri', 'spaceUri', 'message', 'createdAt'])
				.where('requesterDid', '=', viewer.did)
				.where('deletedAt', 'is', null)
				// A withdrawn one's record is gone already (EpkService.unindexRequest).
				.where('status', '!=', 'withdrawn')
				.orderBy('createdAt', 'desc')
				.execute(),
			db
				.selectFrom('epkDraft')
				.select(['uri', 'spaceUri', 'updatedAt'])
				.where('authorDid', '=', viewer.did)
				.orderBy('updatedAt', 'desc')
				.execute()
		]);

		const kitRequests = epkRequests.filter(byRoot);
		const kitDrafts = drafts.filter(byRoot);
		const names = await this.projectNames([...kitRequests, ...kitDrafts].map((row) => row.spaceUri));
		const of = (space: string) => (names.has(space) ? ` for ${names.get(space)}` : '');

		return [
			...requests
				.filter((request) => request.repoDid === rootDid && !request.deletedAt)
				.map((request) => ({
					uri: request.uri,
					kind: 'request' as const,
					title: requestTitle(request),
					excerpt: request.record.message ?? request.record.claim?.evidence ?? null,
					createdAt: request.createdAt
				})),
			...events.filter(byRoot).map((event) => {
				const record = parseJson<Partial<BookingRecord>>(event.record, {});

				return {
					uri: event.uri,
					kind: 'booking' as const,
					title: `${event.kind === 'inquiry' ? 'Inquiry' : 'Message'}${event.title ? ` in ${event.title}` : ''}`,
					excerpt: record.text ?? record.message ?? null,
					createdAt: event.createdAt
				};
			}),
			...kitRequests.map((row) => ({
				uri: row.uri,
				kind: 'epkRequest' as const,
				title: `Press kit access${of(row.spaceUri)}`,
				excerpt: row.message,
				createdAt: row.createdAt
			})),
			...kitDrafts.map((row) => ({
				uri: row.uri,
				kind: 'epkDraft' as const,
				title: `Press kit draft${of(row.spaceUri)}`,
				excerpt: null,
				createdAt: row.updatedAt
			}))
		];
	}

	/** Deletes one record the root wrote for `viewer`. */
	async delete(viewer: Viewer, uri: string): Promise<void> {
		this.limit(viewer);
		await this.remove(viewer, uri);
	}

	/** Deletes every record of `kind` the root wrote for `viewer`. Returns how many went. */
	async deleteAll(viewer: Viewer, kind: HeldRecordKind): Promise<number> {
		this.limit(viewer);
		const held = (await this.list(viewer)).filter((record) => record.kind === kind);

		for (const record of held) await this.remove(viewer, record.uri);

		return held.length;
	}

	/**
	 * Deletes `uri` once both the index and the record itself say the root wrote it for `viewer`: the URI comes from a
	 * form, and the index could lag behind the repo.
	 */
	private async remove(viewer: Viewer, uri: string): Promise<void> {
		const { db, rootDid } = this.core;
		const parts = parseSpaceRecordUri(uri);
		const collection = parts?.author === rootDid ? COLLECTIONS.get(parts.collection) : undefined;
		const kind = collection && KINDS[collection.kind];

		const indexed =
			kind &&
			(await db
				.selectFrom(kind.table)
				.select('uri')
				.where('uri', '=', uri)
				.where(sql.ref(kind.person), '=', viewer.did)
				.$if(kind.keeps, (query) => query.where(sql.ref('deletedAt'), 'is', null))
				.executeTakeFirst());

		if (!parts || !collection || !kind || !indexed) throw new NotFoundError("That isn't one of yours");
		const root = await this.core.root();
		const space = spaceOfRecord(uri)!;

		const stored = await root.getSpaceRecord({
			space,
			repo: rootDid,
			collection: parts.collection,
			rkey: parts.rkey
		});

		if (stored && stored.value.onBehalfOf !== viewer.did) {
			this.core.logger.warn('the index and the repo disagree on whose record this is', { uri, did: viewer.did });
			throw new NotFoundError("That isn't one of yours");
		}

		// One that's gone already only has its index row left.
		if (stored) await root.deleteSpaceRecord({ space, collection: parts.collection, rkey: parts.rkey });
		await this.forget(collection.kind, uri);
		this.core.logger.info('deleted a record held on their behalf', { uri, did: viewer.did });

		if (!stored) return;
		await this.core.audit.record({
			actor: viewer.did,
			actorRole: actorRole(viewer),
			action: 'record.delete',
			subject: uri,
			subjectKind: kind.subject,
			summary: `Deleted ${collection.label} filed on their behalf`,
			details: { collection: parts.collection }
		});
	}

	/**
	 * Takes a deleted record out of the index. A decided request keeps its row without what it said, so the decision
	 * stands (a declined press kit request still can't be asked again); anything else goes, an open request as if
	 * withdrawn.
	 */
	private async forget(kind: HeldRecordKind, uri: string): Promise<void> {
		const { db } = this.core;
		const deletedAt = nowIso();

		if (kind === 'request') {
			const kept = await db
				.updateTable('request')
				.set({ record: sql<string>`json_remove(record, '$.profile', '$.details', '$.claim', '$.message')`, deletedAt })
				.where('uri', '=', uri)
				.where('status', 'not in', ['pending', 'changes_requested'])
				.executeTakeFirst();

			if (kept.numUpdatedRows) return;
		}

		if (kind === 'epkRequest') {
			const kept = await db
				.updateTable('epkRequest')
				.set({ role: null, message: null, deletedAt })
				.where('uri', '=', uri)
				.where('status', 'in', ['approved', 'declined'])
				.executeTakeFirst();

			if (kept.numUpdatedRows) return;
		}

		await db.deleteFrom(KINDS[kind].table).where('uri', '=', uri).execute();
	}

	/** The project each press kit request or draft space belongs to, by name, where it's known. */
	private async projectNames(spaces: string[]): Promise<Map<string, string>> {
		const projectOf = new Map(
			spaces.flatMap((space) => {
				const ref = parseSpaceUri(space);
				const perProject = ref?.type === SPACE_TYPE.epkRequests || ref?.type === SPACE_TYPE.backstage;

				return perProject ? [[space, publicRecordUri(this.core.rootDid, COLLECTION.project, ref.skey)] as const] : [];
			})
		);

		if (projectOf.size === 0) return new Map();
		const projects = await this.projects.byUris([...projectOf.values()]);

		return new Map(
			[...projectOf].flatMap(([space, uri]) => {
				const project = projects.get(uri);

				return project ? [[space, project.profile.name] as const] : [];
			})
		);
	}

	private limit(viewer: Viewer): void {
		if (!this.limiter.take(`held-records:${viewer.did}`))
			throw new UserError('Too many deletes. Try again in a minute.', 429);
	}
}
