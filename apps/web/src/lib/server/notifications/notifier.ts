import { TID } from '@atproto/common-web';
import type { NotificationView } from '#lib/types.ts';
import { type Db, nowIso } from '../db';
import type { NotificationRow } from '../db/schema';
import { safeNext } from '../http';
import type { Logger } from '../logger';
import { isNotificationKind, KINDS, type NotificationData, type NotificationKind } from './kinds';

const DAY_MS = 86_400_000;

/**
 * How old something can be and still notify anyone. A sync of a wiped database, or of a space the site only just
 * heard of, replays every old record, and those aren't news.
 */
const FRESH_MS = 14 * DAY_MS;

/** Whether something that happened `at` is still news. False for a date that doesn't parse, which nobody could tell. */
export const isFresh = (at: string): boolean => Date.now() - Date.parse(at) < FRESH_MS;

/** How long an item stays in the inbox once it's read. */
const KEEP_READ_MS = 90 * DAY_MS;

/** How long it stays there unread. */
const KEEP_UNREAD_MS = 180 * DAY_MS;

export type NotifyInput<K extends NotificationKind> = {
	kind: K;
	/** Who to tell, leaving out whoever did it. */
	to: Iterable<string>;
	/** What notified them, like a record's URI. Each recipient hears of each key once. */
	key: string;
	/** The thread, event or space it's about. Opening it there marks the item read. */
	subject: string;
	data: NotificationData<K>;
	/** The site path the item opens. */
	href: string;
	/** When it happened. */
	at: string;
};

/** Generic words for an item whose kind or data this version doesn't know, which still opens what it's about. */
const UNKNOWN = { title: 'Something changed' };

function view(row: NotificationRow): NotificationView {
	const kind = isNotificationKind(row.kind) ? KINDS[row.kind] : null;
	const rendered = kind?.view(row.data);

	return {
		id: row.id,
		topic: rendered && kind ? kind.topic : null,
		...(rendered ?? UNKNOWN),
		href: row.href,
		occurredAt: row.occurredAt,
		read: row.readAt !== null
	};
}

/**
 * People's inboxes. Every method that reads or changes items takes the recipient's DID and only touches their items,
 * so nobody can read or mark anyone else's.
 */
export class Notifier {
	constructor(private readonly deps: { db: Db; logger: Logger }) {}

	/** Puts an item in the inbox of everyone in `to` who hasn't had `key` yet. Returns how many it made. */
	async notify<K extends NotificationKind>(input: NotifyInput<K>): Promise<number> {
		const recipients = [...new Set(input.to)];

		if (!isFresh(input.at) || recipients.length === 0) return 0;
		const data = JSON.stringify(KINDS[input.kind].data.parse(input.data));
		const createdAt = nowIso();

		const result = await this.deps.db
			.insertInto('notification')
			.values(
				recipients.map((recipientDid) => ({
					id: TID.nextStr(),
					recipientDid,
					kind: input.kind,
					dedupeKey: input.key,
					subjectUri: input.subject,
					href: safeNext(input.href, '/dashboard/inbox'),
					data,
					occurredAt: input.at,
					createdAt,
					readAt: null
				}))
			)
			.onConflict((oc) => oc.columns(['recipientDid', 'dedupeKey']).doNothing())
			.executeTakeFirst();

		const created = Number(result.numInsertedOrUpdatedRows ?? 0);

		if (created > 0) this.deps.logger.debug({ kind: input.kind, key: input.key, created }, 'notified');

		return created;
	}

	/** `did`'s items, newest first, `limit` at a time, from the one after the item `before`. */
	async list(
		did: string,
		{ before, limit = 50 }: { before?: string; limit?: number } = {}
	): Promise<NotificationView[]> {
		const { db } = this.deps;
		let query = db.selectFrom('notification').selectAll().where('recipientDid', '=', did);

		const cursor = before
			? await db
					.selectFrom('notification')
					.select(['occurredAt', 'id'])
					.where('recipientDid', '=', did)
					.where('id', '=', before)
					.executeTakeFirst()
			: undefined;

		if (cursor) {
			query = query.where((eb) => eb(eb.refTuple('occurredAt', 'id'), '<', eb.tuple(cursor.occurredAt, cursor.id)));
		}

		const rows = await query.orderBy('occurredAt', 'desc').orderBy('id', 'desc').limit(limit).execute();

		return rows.map(view);
	}

	async unreadCount(did: string): Promise<number> {
		const { count } = await this.deps.db
			.selectFrom('notification')
			.select((eb) => eb.fn.countAll<number>().as('count'))
			.where('recipientDid', '=', did)
			.where('readAt', 'is', null)
			.executeTakeFirstOrThrow();

		return Number(count);
	}

	/** Marks one of `did`'s items read, returning where it leads, or null when they have no such item. */
	async open(did: string, id: string): Promise<string | null> {
		const { db } = this.deps;

		const row = await db
			.selectFrom('notification')
			.select('href')
			.where('recipientDid', '=', did)
			.where('id', '=', id)
			.executeTakeFirst();

		if (!row) return null;
		await this.markRead(did, { id });

		return row.href;
	}

	async markAllRead(did: string): Promise<void> {
		await this.markRead(did);
	}

	/** Marks read whatever `did` was told about `subject`, once they've seen it there. */
	async markReadBySubject(did: string, subject: string): Promise<void> {
		await this.markRead(did, { subjectUri: subject });
	}

	/** Marks `did`'s unread items read, only the one with `id` or those about `subjectUri` when given. */
	private async markRead(did: string, only: { id?: string; subjectUri?: string } = {}): Promise<void> {
		let query = this.deps.db
			.updateTable('notification')
			.set({ readAt: nowIso() })
			.where('recipientDid', '=', did)
			.where('readAt', 'is', null);

		if (only.id) query = query.where('id', '=', only.id);

		if (only.subjectUri) query = query.where('subjectUri', '=', only.subjectUri);
		await query.execute();
	}

	/** Deletes items read longer ago than the inbox keeps them, and unread ones far older. Returns how many. */
	async prune(now = Date.now()): Promise<number> {
		const before = (ms: number) => new Date(now - ms).toISOString();

		const result = await this.deps.db
			.deleteFrom('notification')
			.where((eb) =>
				eb.or([
					eb.and([eb('readAt', 'is not', null), eb('createdAt', '<', before(KEEP_READ_MS))]),
					eb('createdAt', '<', before(KEEP_UNREAD_MS))
				])
			)
			.executeTakeFirst();

		const deleted = Number(result.numDeletedRows);

		if (deleted > 0) this.deps.logger.info({ deleted }, 'pruned old inbox items');

		return deleted;
	}
}
