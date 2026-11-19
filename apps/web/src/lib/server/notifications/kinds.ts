import type { z } from 'zod';
import type { NotificationTopic } from '#lib/types.ts';
import { KIND_FILES } from '#notification-kinds';
import type { Kinds } from './kinds.generated.ts';

const fileKind = (path: string) => path.slice(path.lastIndexOf('/') + 1, -'.ts'.length);

/** Every kind of notification, one per file in kinds/ and named by it, so a new kind is a new file. */
// SAFETY: scripts/notification-kinds.ts generates Kinds from these same files.
export const KINDS = Object.fromEntries(
	Object.entries(KIND_FILES).map(([path, definition]) => [fileKind(path), definition])
) as Kinds;

export type NotificationKind = keyof typeof KINDS;

/** The topics some kind is about, which are the ones worth offering a choice of. */
export const KIND_TOPICS: NotificationTopic[] = [...new Set(Object.values(KINDS).map((kind) => kind.topic))];

/** The data a kind's producer passes, before its schema fills anything in. */
export type NotificationData<K extends NotificationKind> = z.input<(typeof KINDS)[K]['data']>;

export const isNotificationKind = (value: string): value is NotificationKind => Object.hasOwn(KINDS, value);
