import { AtUri, isValidAtUri, isValidDid } from '@atproto/syntax';
import type { DjActivity } from '#lib/dj.ts';
import type { PageMeta } from '#lib/meta.ts';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProfileView, ProjectView } from '#lib/types.ts';
import type { RequestService } from './services/requests';

/**
 * The records behind a page, for its at-tags. Canonical is the record whose deletion takes the page down, alternate a
 * record it shows but outlives, and author a person who put themselves on it.
 */
export type PageRecords = NonNullable<PageMeta['records']>;

/** Only addresses that name their repo by DID, since a handle can move to another account; each once. */
const byDid = (uris: (string | undefined)[]) => [
	...new Set(uris.filter((uri): uri is string => isValidAtUri(uri) && isValidDid(new AtUri(uri).host)))
];

/**
 * A performer's page's records: the project by default. Its authors are the managers who asked to manage it
 * themselves, so nobody an admin named without asking is credited with it.
 */
export async function projectRecords(
	requests: Pick<RequestService, 'joinedManagers'>,
	project: Pick<ProjectView, 'uri' | 'managers'>,
	{ canonical = project.uri, alternate = [] }: { canonical?: string; alternate?: (string | undefined)[] } = {}
): Promise<PageRecords> {
	const joined = await requests.joinedManagers(project);

	return {
		canonical: byDid([canonical]),
		alternate: byDid(alternate),
		author: byDid(project.managers.filter((did) => joined.has(did)).map((did) => `at://${did}`))
	};
}

/**
 * The managers' DJ stats records, when they're already at hand. Streamed ones are left out rather than waited for:
 * the head is written before they arrive.
 */
export const djProfiles = (dj: DjActivity[] | Promise<DjActivity[]>): string[] =>
	Array.isArray(dj)
		? dj.flatMap((entry) => (entry.profile ? [`at://${entry.did}/${COLLECTION.djProfile}/self`] : []))
		: [];

/** A person's profile page's records: their profile record, which they wrote, when they have one. */
export const profileRecords = (profile: Pick<ProfileView, 'did'> | null): PageRecords => ({
	canonical: profile ? byDid([`at://${profile.did}/${COLLECTION.profile}/self`]) : [],
	alternate: [],
	author: profile ? byDid([`at://${profile.did}`]) : []
});
