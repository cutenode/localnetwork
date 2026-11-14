import { AtUri, isValidAtUri, isValidDid } from '@atproto/syntax';
import { COLLECTION } from '#lib/nsid.ts';
import type { PerformerRef } from '#lib/types.ts';

/** The record key of a DJ's profile, which names them as a performer. */
const PROFILE_KEY = 'self';

/**
 * The URI that names a performer: a DJ's profile, an act's listing in the root's repo (keyed by its space's key, so it
 * names the act whether it's listed or not), or a project in the root's repo.
 */
export function performerUri(rootDid: string, ref: PerformerRef): string {
	switch (ref.kind) {
		case 'dj':
			return `at://${ref.did}/${COLLECTION.profile}/${PROFILE_KEY}`;
		case 'act':
			return `at://${rootDid}/${COLLECTION.listing}/${ref.skey}`;
		case 'project':
			return `at://${rootDid}/${COLLECTION.project}/${ref.rkey}`;
	}
}

/** The performer `uri` names, or null when it names none. Acts and projects only count in the root's repo. */
export function parsePerformerUri(rootDid: string, uri: string): PerformerRef | null {
	if (!isValidAtUri(uri)) return null;
	const parsed = new AtUri(uri);

	if (parsed.isSpace || !isValidDid(parsed.host) || !parsed.rkey) return null;

	if (parsed.collection === COLLECTION.profile)
		return parsed.rkey === PROFILE_KEY ? { kind: 'dj', did: parsed.host } : null;

	if (parsed.host !== rootDid) return null;

	if (parsed.collection === COLLECTION.listing) return { kind: 'act', skey: parsed.rkey };

	return parsed.collection === COLLECTION.project ? { kind: 'project', rkey: parsed.rkey } : null;
}
