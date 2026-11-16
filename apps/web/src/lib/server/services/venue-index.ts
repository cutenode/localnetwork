import type { JsonObject } from '@atproto/lex-json';
import { COLLECTION } from '#lib/nsid.ts';
import type { VenueRecord } from '#lib/types.ts';
import { isValidRecord } from '../atproto/lexicons';
import { recordCid } from '../atproto/repo-client';
import { parseSpaceUri } from '../atproto/spaces';
import { type Db, parseJson } from '../db';
import type { ListingSource } from './listings';

/** The venue record's one key in each venue space. */
export const VENUE_KEY = 'self';

/** A name or city as two people typing the same place would agree on: any case, any spacing. */
export const nameKey = (text: string) => text.trim().replace(/\s+/g, ' ').toLowerCase();

/**
 * Indexes the root's venue record in a venue space, or returns false when it isn't one this app reads. Callers check
 * the root wrote it, in a venue space under the root, at its key.
 */
export async function indexVenue(db: Db, space: string, value: JsonObject): Promise<boolean> {
	const skey = parseSpaceUri(space)?.skey;

	if (!isValidRecord<VenueRecord>(COLLECTION.venue, value) || !skey) return false;
	const record = value;

	const row = {
		skey,
		fsqPlaceId: record.fsq?.fsq_place_id ?? null,
		name: record.name,
		nameKey: nameKey(record.name),
		locality: record.address.locality ?? null,
		country: record.address.country,
		record: JSON.stringify(record),
		cid: await recordCid({ ...value, $type: COLLECTION.venue }),
		updatedAt: record.updatedAt
	};

	await db
		.insertInto('venue')
		.values({ spaceUri: space, ...row })
		.onConflict((oc) => oc.column('spaceUri').doUpdateSet(row))
		.execute();

	return true;
}

/**
 * What a venue's listing shows: its name and where it is, which are public since it's a public place. The listing's
 * key is the venue's space key, which is what its page goes by. How it takes events and its Foursquare match stay in
 * its space.
 */
export function venueListingSource(db: Db): ListingSource {
	return {
		async publicFields(space) {
			const row = await db.selectFrom('venue').select('record').where('spaceUri', '=', space).executeTakeFirst();
			const record = parseJson<VenueRecord | null>(row?.record, null);

			if (!record) return null;
			const { name, address, geo } = record;
			const fields: JsonObject = JSON.parse(JSON.stringify({ address, geo }));

			return { name, fields };
		}
	};
}
