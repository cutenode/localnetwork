import type { InboxSubjectView } from '#lib/types.ts';
import type { NotificationRow } from '../db/schema';
import { capitalized, isNotificationKind, KINDS, type Line } from './kinds';

/** Generic words for an item whose kind or data this version doesn't know. */
const UNKNOWN: Line = {
	subject: 'Something changed',
	scope: 'Local Network',
	who: '',
	did: (count) => (count === 1 ? 'something changed' : `${count} things changed`)
};

const and = new Intl.ListFormat('en', { type: 'conjunction' });

const lineOf = (row: NotificationRow): Line | null =>
	isNotificationKind(row.kind) ? KINDS[row.kind].lineOf(row.data) : null;

/**
 * One subject's inbox row from its items, newest first. The sentence covers the unread items, or the newest once all
 * are read, saying what each side did in the order they started: "The promoter sent 2 messages and changed the status
 * to Negotiating".
 */
export function summarize(rows: [NotificationRow, ...NotificationRow[]]): InboxSubjectView {
	const [newest] = rows;
	const lines = rows.map((row) => ({ row, line: lineOf(row) ?? UNKNOWN }));
	const known = lines.find(({ line }) => line !== UNKNOWN);
	const unread = lines.filter(({ row }) => row.readAt === null);

	// By who did it, then by kind. A Map keeps the order each first turned up, and set() keeps the newest line of a kind.
	const told = new Map<string, Map<string, { line: Line; count: number }>>();

	for (const { row, line } of (unread.length ? unread : lines.slice(0, 1)).toReversed()) {
		const kinds = told.get(line.who) ?? new Map<string, { line: Line; count: number }>();
		kinds.set(row.kind, { line, count: (kinds.get(row.kind)?.count ?? 0) + 1 });
		told.set(line.who, kinds);
	}

	const clauses = [...told].map(([who, kinds]) => {
		const did = and.format([...kinds.values()].map(({ line, count }) => line.did(count)));

		return who ? `${who} ${did}` : did;
	});

	const named = known?.line ?? UNKNOWN;

	return {
		id: newest.id,
		topic: known && isNotificationKind(known.row.kind) ? KINDS[known.row.kind].topic : null,
		title: named.subject,
		scope: named.scope,
		summary: capitalized(and.format(clauses)),
		href: newest.href,
		occurredAt: newest.occurredAt,
		unread: unread.length > 0
	};
}
