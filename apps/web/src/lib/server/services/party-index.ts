import type { JsonObject } from '@atproto/lex-json';
import { COLLECTION } from '#lib/nsid.ts';
import type { PartyRecord } from '#lib/types.ts';
import { isValidRecord } from '../atproto/lexicons';
import { recordCid } from '../atproto/repo-client';
import { parseSpaceUri } from '../atproto/spaces';
import { type Db, parseJson } from '../db';
import type { ListingSource } from './listings';

/** The party record's one key in each party space. */
export const PARTY_KEY = 'self';

/** `value` as a party record, or null when it isn't valid by the party lexicon, which PartyRecord mirrors. */
export function partyRecord(value: JsonObject | undefined): PartyRecord | null {
	return value && isValidRecord<PartyRecord>(COLLECTION.party, value) ? value : null;
}

/**
 * Indexes the root's party record in a party space, or returns false when it isn't one this app reads. Callers check
 * the root wrote it, in a party space under the root, at its key.
 */
export async function indexParty(db: Db, space: string, value: JsonObject): Promise<boolean> {
	const record = partyRecord(value);
	const skey = parseSpaceUri(space)?.skey;

	if (!record || !skey) return false;

	const row = {
		skey,
		name: record.basics.name,
		publishedAt: record.publishedAt ?? null,
		record: JSON.stringify(record),
		cid: await recordCid({ ...value, $type: COLLECTION.party }),
		updatedAt: record.updatedAt
	};

	await db
		.insertInto('party')
		.values({ spaceUri: space, ...row })
		.onConflict((oc) => oc.column('spaceUri').doUpdateSet(row))
		.execute();

	return true;
}

/**
 * What a listed party's listing shows: its name and basics. A draft has nothing to list. Its hosts and cohosts who
 * haven't hidden come from its people. The avatar is a blob the root uploaded when the page was saved, so the root's
 * public listing can hold it. The listing's key is the party's space key, which is what its page goes by.
 */
export function partyListingSource(db: Db): ListingSource {
	return {
		async publicFields(space) {
			const row = await db.selectFrom('party').select('record').where('spaceUri', '=', space).executeTakeFirst();
			const record = parseJson<PartyRecord | null>(row?.record, null);

			if (!record?.publishedAt) return null;
			const { basics } = record;
			const fields: JsonObject = JSON.parse(JSON.stringify({ basics }));

			return { name: basics.name, fields };
		}
	};
}
