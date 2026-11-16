import type { z } from 'zod';
import type { NotificationTopic } from '#lib/types.ts';
import { parseJson } from '../db';

/** An inbox item in words. */
type Rendered = { title: string; body?: string };

/**
 * An item's part in its subject's inbox row: what the subject is called, whose initials stand for it, who acted, and
 * what they did, `count` times when there's more than one of this kind from them, this item being the latest.
 */
export type Line = { subject: string; scope: string; who: string; did: (count: number) => string };

/** `text` with its first letter capital, to start a sentence. */
export const capitalized = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/**
 * A kind of notification: its topic, the data it keeps, and how that reads. Items are put into words when they're
 * read rather than when they're made, so a change of copy reaches the items already there.
 */
export function kind<S extends z.ZodType>(definition: {
	topic: NotificationTopic;
	data: S;
	render: (data: z.output<S>) => Rendered;
	line: (data: z.output<S>) => Line;
}) {
	/** The item's data from its stored JSON, or null when that no longer fits the kind (a later version changed it). */
	const parse = (json: string) => {
		const parsed = definition.data.safeParse(parseJson(json, null));

		return parsed.success ? parsed.data : null;
	};

	return {
		...definition,
		view(json: string): Rendered | null {
			const data = parse(json);

			return data && definition.render(data);
		},
		lineOf(json: string): Line | null {
			const data = parse(json);

			return data && definition.line(data);
		}
	};
}

/** A kind as kind() makes it, whatever data it keeps. */
export type Kind = ReturnType<typeof kind>;
