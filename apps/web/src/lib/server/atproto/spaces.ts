import { type AtUriParts, isValidDid, parseAtUriString } from '@atproto/syntax';
import { SPACE_KINDS, type SpaceKind } from '#lib/capabilities.ts';
import { ROOT_SPACE_KEY, SELF_SPACE_KEY, SPACE_TYPE, type SpaceType } from '#lib/nsid.ts';

export type SpaceRefParts = { authority: string; type: string; skey: string };

export type SpaceRecordParts = SpaceRefParts & { author: string; collection: string; rkey: string };

/** A valid AT URI's parts, or null. The syntax allows a `#` fragment, but no space or record URI here has one. */
function atUriParts(uri: string): AtUriParts | null {
	const parsed = parseAtUriString(uri);

	return parsed.success && parsed.value.hash === undefined ? parsed.value : null;
}

export function spaceUri(authority: string, type: string, skey: string): string {
	return `at://${authority}/space/${type}/${skey}`;
}

export function parseSpaceUri(uri: string): SpaceRefParts | null {
	const parts = atUriParts(uri);

	if (!parts?.isSpace || parts.author) return null;

	return { authority: parts.authority, type: parts.spaceType, skey: parts.skey };
}

export function spaceRecordUri(space: string, author: string, collection: string, rkey: string): string {
	return `${space}/${author}/${collection}/${rkey}`;
}

export function parseSpaceRecordUri(uri: string): SpaceRecordParts | null {
	const parts = atUriParts(uri);

	if (!parts?.isSpace || !parts.author || !parts.rkey) return null;

	return {
		authority: parts.authority,
		type: parts.spaceType,
		skey: parts.skey,
		author: parts.author,
		collection: parts.collection,
		rkey: parts.rkey
	};
}

export function spaceOfRecord(uri: string): string | null {
	const parts = parseSpaceRecordUri(uri);

	return parts ? spaceUri(parts.authority, parts.type, parts.skey) : null;
}

export function publicRecordUri(did: string, collection: string, rkey: string): string {
	return `at://${did}/${collection}/${rkey}`;
}

/** A record URI's last segment, its rkey (or a space URI's, its skey). */
export function rkeyOf(uri: string): string {
	return uri.slice(uri.lastIndexOf('/') + 1);
}

export function parsePublicRecordUri(uri: string): { did: string; collection: string; rkey: string } | null {
	const parts = atUriParts(uri);

	// A public URI's authority may be a handle; a record URI here names its repo by DID.
	if (!parts || parts.isSpace || !parts.rkey || !isValidDid(parts.authority)) return null;

	return { did: parts.authority, collection: parts.collection, rkey: parts.rkey };
}

/** The spaces Local Network keeps under its root account. */
export type RootSpaces = {
	audit: string;
	moderation: string;
	staff: string;
	industry: string;
};

export function rootSpaces(rootDid: string): RootSpaces {
	return {
		audit: spaceUri(rootDid, SPACE_TYPE.audit, ROOT_SPACE_KEY),
		moderation: spaceUri(rootDid, SPACE_TYPE.moderation, ROOT_SPACE_KEY),
		staff: spaceUri(rootDid, SPACE_TYPE.staff, ROOT_SPACE_KEY),
		industry: spaceUri(rootDid, SPACE_TYPE.industry, ROOT_SPACE_KEY)
	};
}

export function bookingSpace(rootDid: string, skey: string): string {
	return spaceUri(rootDid, SPACE_TYPE.booking, skey);
}

export function plannerSpace(userDid: string): string {
	return spaceUri(userDid, SPACE_TYPE.planner, SELF_SPACE_KEY);
}

/** A press kit's space, with the press kit record's key: under the root, or for one made before the root held press kits, under the manager who made it. */
export function epkSpace(authority: string, rkey: string): string {
	return spaceUri(authority, SPACE_TYPE.epk, rkey);
}

/** Where people ask to see a project's press kits: under the root, with the project record's key. */
export function epkRequestSpace(rootDid: string, projectRkey: string): string {
	return spaceUri(rootDid, SPACE_TYPE.epkRequests, projectRkey);
}

/** A project's backstage: under the root, with the project record's key. */
export function backstageSpace(rootDid: string, projectRkey: string): string {
	return spaceUri(rootDid, SPACE_TYPE.backstage, projectRkey);
}

/** The space type of each kind of shared space: a performer's, party's or venue's, under the root. */
export const SHARED_SPACE_TYPES = {
	performer: SPACE_TYPE.performer,
	party: SPACE_TYPE.party,
	venue: SPACE_TYPE.venue
} as const satisfies Record<SpaceKind, SpaceType>;

/** A performer's, party's or venue's space: under the root, with a key of its own. */
export function sharedSpace(rootDid: string, kind: SpaceKind, skey: string): string {
	return spaceUri(rootDid, SHARED_SPACE_TYPES[kind], skey);
}

/** The kind of shared space a space type is for, or null for any other type. */
export function sharedSpaceKind(type: string): SpaceKind | null {
	return SPACE_KINDS.find((kind) => SHARED_SPACE_TYPES[kind] === type) ?? null;
}
