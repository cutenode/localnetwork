import { AtUri, isValidAtUri, isValidDid } from '@atproto/syntax';
import type { DjActivity } from '#lib/dj.ts';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProfileView, ProjectView } from '#lib/types.ts';
import type { RequestService } from './services/requests';

/**
 * The records behind a page, for its at-tags. Canonical is the record whose deletion takes the page down, alternate a
 * record it shows but outlives, and author a person who put themselves on it.
 */
export type PageRecords = { canonical: string[]; alternate: string[]; author: string[] };

/** `value` as a double-quoted attribute can hold it. */
const attribute = (value: string) =>
	value.replaceAll('&', '&amp;').replaceAll('"', '&quot;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');

/**
 * A page's records as at-tags (tangled.org/chrisshank.com/at-tags), one `<meta name="at:…">` per record, for the server
 * hook to write into its head. Every value is escaped, though byDid only lets through URIs that parse.
 */
export const recordTags = (records: PageRecords) =>
	(['canonical', 'alternate', 'author'] as const)
		.flatMap((name) => records[name].map((uri) => `<meta name="at:${name}" content="${attribute(uri)}" />`))
		.join('');

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

/** A DJ's set's page's records: the set, which they wrote. */
export const setRecords = (uri: string, did: string): PageRecords => ({
	canonical: byDid([uri]),
	alternate: [],
	author: byDid([`at://${did}`])
});

/** A person's profile page's records: their profile record, which they wrote, when they have one. */
export const profileRecords = (profile: Pick<ProfileView, 'did'> | null): PageRecords => ({
	canonical: profile ? byDid([`at://${profile.did}/${COLLECTION.profile}/self`]) : [],
	alternate: [],
	author: profile ? byDid([`at://${profile.did}`]) : []
});
