// What a list in storage is, read back.
//
// The session queue, the Manager's list and the artists list are each an array
// of entries in chrome.storage, and none of them is this version's to trust: a
// list written by another version, or restored from an imported state file, can
// hold shapes nothing here expects. Each store names the one field an entry
// cannot go without, and takes the rest as it was written.

import type { Track } from '$lib/types';
import { fallback, z } from './zod.ts';

/** Anything with a track's path on it, which is all a stored track is checked for. */
const Permalinked = z.looseObject({ permalink: z.string().check(z.minLength(1)) });

/** A stored track: on trust past its permalink. */
export const storedTrack = <T extends Track>() => z.custom<T>((item) => Permalinked.safeParse(item).success);

/**
 * A stored list as the entries in it that `entry` takes, in order. Whatever else is in it is
 * dropped, and anything that is not a list at all is an empty one.
 *
 * @returns the parse, as a function that can be handed around (a schema's own reads `this`)
 */
export function storedList<T>(entry: z.ZodMiniType<T>) {
	const list = z.pipe(
		fallback(z.array(fallback(z.nullable(entry), null)), []),
		z.transform((items: (T | null)[]) => items.filter((item) => item !== null))
	);

	return list.parse.bind(list);
}
