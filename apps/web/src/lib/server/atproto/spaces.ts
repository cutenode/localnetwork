import { ROOT_SPACE_KEY, SELF_SPACE_KEY, SPACE_TYPE, type SpaceType } from '#lib/nsid.ts';

export type SpaceRefParts = { authority: string; type: string; skey: string };

export type SpaceRecordParts = SpaceRefParts & { author: string; collection: string; rkey: string };

const SPACE_URI = /^at:\/\/(did:[a-z0-9]+:[a-zA-Z0-9._:%-]+)\/space\/([a-zA-Z0-9.-]+)\/([A-Za-z0-9._:~-]{1,512})$/;

const SPACE_RECORD_URI =
	/^at:\/\/(did:[a-z0-9]+:[a-zA-Z0-9._:%-]+)\/space\/([a-zA-Z0-9.-]+)\/([A-Za-z0-9._:~-]{1,512})\/(did:[a-z0-9]+:[a-zA-Z0-9._:%-]+)\/([a-zA-Z0-9.-]+)\/([A-Za-z0-9._:~-]{1,512})$/;

const PUBLIC_RECORD_URI = /^at:\/\/(did:[a-z0-9]+:[a-zA-Z0-9._:%-]+)\/([a-zA-Z0-9.-]+)\/([A-Za-z0-9._:~-]{1,512})$/;

export function spaceUri(authority: string, type: string, skey: string): string {
	return `at://${authority}/space/${type}/${skey}`;
}

export function parseSpaceUri(uri: string): SpaceRefParts | null {
	const match = SPACE_URI.exec(uri);

	return match ? { authority: match[1], type: match[2], skey: match[3] } : null;
}

export function spaceRecordUri(space: string, author: string, collection: string, rkey: string): string {
	return `${space}/${author}/${collection}/${rkey}`;
}

export function parseSpaceRecordUri(uri: string): SpaceRecordParts | null {
	const match = SPACE_RECORD_URI.exec(uri);

	return match
		? {
				authority: match[1],
				type: match[2],
				skey: match[3],
				author: match[4],
				collection: match[5],
				rkey: match[6]
			}
		: null;
}

export function spaceOfRecord(uri: string): string | null {
	const parts = parseSpaceRecordUri(uri);

	return parts ? spaceUri(parts.authority, parts.type, parts.skey) : null;
}

export function publicRecordUri(did: string, collection: string, rkey: string): string {
	return `at://${did}/${collection}/${rkey}`;
}

export function parsePublicRecordUri(uri: string): { did: string; collection: string; rkey: string } | null {
	const match = PUBLIC_RECORD_URI.exec(uri);

	if (!match || match[2] === 'space') return null;

	return { did: match[1], collection: match[2], rkey: match[3] };
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

/** A press kit's space: under its owner, with the press kit record's key. */
export function epkSpace(ownerDid: string, rkey: string): string {
	return spaceUri(ownerDid, SPACE_TYPE.epk, rkey);
}

export function isSpaceType(value: string): value is SpaceType {
	return Object.values<string>(SPACE_TYPE).includes(value);
}
