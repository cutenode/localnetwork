import type { z } from 'zod';
import type { NotificationTopic } from '#lib/types.ts';
import { parseJson } from '../db';

/** An inbox item in words. */
type Rendered = { title: string; body?: string };

/**
 * A kind of notification: its topic, the data it keeps, and how that reads. Items are put into words when they're
 * read rather than when they're made, so a change of copy reaches the items already there.
 */
export function kind<S extends z.ZodType>(definition: {
	topic: NotificationTopic;
	data: S;
	render: (data: z.output<S>) => Rendered;
}) {
	return {
		...definition,
		/** The item in words from its stored JSON, or null when that no longer fits the kind (a later version changed it). */
		view(json: string): Rendered | null {
			const parsed = definition.data.safeParse(parseJson(json, null));

			return parsed.success ? definition.render(parsed.data) : null;
		}
	};
}

/** A kind as kind() makes it, whatever data it keeps. */
export type Kind = ReturnType<typeof kind>;
