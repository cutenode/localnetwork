import type { z } from 'zod';
import type { djProfileSchema, djSetSchema } from './dj-schema.ts';

/**
 * What locallibrary publishes about a DJ, read from their own repo: `computer.localnetwork.dj.profile`
 * (one per account, their playing stats) and `computer.localnetwork.dj.set` (one per set played).
 * The schemas live in dj-schema.ts, for the server; this module is what pages need to show them.
 */
export const DJ_PROFILE = 'computer.localnetwork.dj.profile';

export const DJ_SET = 'computer.localnetwork.dj.set';

export type DjProfile = z.infer<typeof djProfileSchema>;

export type DjSet = z.infer<typeof djSetSchema> & { uri: string };

/** One account's records: what a project page and its press kit show for each of its DJs. */
export type DjActivity = { did: string; handle: string | null; profile: DjProfile | null; sets: DjSet[] };

/** "40+" for a rounded count, "48" for an exact one. */
export function countLabel(value: NonNullable<DjProfile['sets']>['total']): string | null {
	if (!value) return null;

	return 'count' in value ? value.count.toLocaleString('en') : `${value.atLeast.toLocaleString('en')}+`;
}

/** "Halle Nord, Berlin" from what a set says about where it was. */
export function setPlace(set: Pick<DjSet, 'venue' | 'location'>): string | null {
	const parts = [set.venue ?? set.location?.name, set.location?.locality].filter((part): part is string =>
		Boolean(part)
	);

	return parts.length ? [...new Set(parts)].join(', ') : null;
}

/** A track's BPM from the record's hundredths, to one decimal when it isn't whole. */
export function trackBpm(bpmCentis: number | undefined): string | null {
	if (bpmCentis === undefined) return null;

	const value = bpmCentis / 100;

	return Number.isInteger(value) ? String(value) : value.toFixed(1);
}
