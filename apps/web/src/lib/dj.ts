import type { z } from 'zod';
import type { djProfileSchema, djSetSchema } from './dj-schema.ts';
import type { localnetwork } from './lexicons/computer.ts';
import type { LexiconRead } from './types.ts';

/**
 * What locallibrary publishes about a DJ to their own repo, as HappyView serves it: `computer.localnetwork.dj.profile`
 * (one per account, their playing stats) and `computer.localnetwork.dj.set` (one per set played).
 * The schemas live in dj-schema.ts, for the server; this module is what pages need to show them.
 */
export type DjProfile = LexiconRead<z.infer<typeof djProfileSchema>, localnetwork.dj.profile.Main>;

export type DjSet = LexiconRead<z.infer<typeof djSetSchema>, localnetwork.dj.set.Main> & { uri: string };

/** One account's records: what a project page and its press kit show for each of its DJs. */
export type DjActivity = { did: string; handle: string | null; profile: DjProfile | null; sets: DjSet[] };

/** What the dashboard home's locallibrary card says about the viewer's own records. */
export type LocallibraryStatus = {
	/** When their stats were last published, or null when they have none. */
	statsAt: string | null;
	/** Their newest set's page and name, and when they last published a set, or null when they have none. */
	sets: { href: string; name: string; at: string | null } | null;
};

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

/** What a set goes by: its title, else where it was played. */
export const setName = (set: Pick<DjSet, 'title' | 'venue' | 'location'>): string =>
	set.title ?? setPlace(set) ?? 'Untitled set';

/** A track's BPM from the record's hundredths, to one decimal when it isn't whole. */
export function trackBpm(bpmCentis: number | undefined): string | null {
	if (bpmCentis === undefined) return null;

	const value = bpmCentis / 100;

	return Number.isInteger(value) ? String(value) : value.toFixed(1);
}
