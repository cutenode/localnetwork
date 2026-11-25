import type { JsonObject } from '@atproto/lex-json';
import { COLLECTION } from '#lib/nsid.ts';
import { EVENT_STATUS } from '#lib/shows.ts';
import type { EventRecord } from '#lib/types.ts';
import { isValidRecord } from '../atproto/lexicons';
import { recordCid } from '../atproto/repo-client';
import type { Db } from '../db';

/** A venue's approval record (`computer.localnetwork.venue.approval`). */
type VenueApprovalRecord = { event: string; approvedBy: string; createdAt: string };

/**
 * Indexes the root's event record, at `uri` in a party's space, or returns false when it isn't valid. Callers check
 * the root wrote it, in a party space under the root. `cid` is a synced record's, left out for one just written.
 */
export async function indexEvent(
	db: Db,
	space: string,
	rkey: string,
	uri: string,
	value: JsonObject,
	cid?: string
): Promise<boolean> {
	if (!isValidRecord<EventRecord>(COLLECTION.event, value)) return false;

	const row = {
		spaceUri: space,
		rkey,
		venueSpace: value.venue ?? null,
		startsAt: value.startsAt ?? null,
		// A record without a status is scheduled, the calendar event status's default.
		status: value.status ?? EVENT_STATUS.scheduled,
		audience: value.audience,
		publishedAt: value.publishedAt ?? null,
		record: JSON.stringify(value),
		cid: cid ?? (await recordCid({ ...value, $type: COLLECTION.event })),
		updatedAt: value.updatedAt
	};

	await db
		.insertInto('event')
		.values({ uri, ...row })
		.onConflict((oc) => oc.column('uri').doUpdateSet(row))
		.execute();

	return true;
}

/**
 * Indexes the root's approval record in a venue's space, or returns false when it isn't valid. Callers check the root
 * wrote it, in a venue space under the root.
 */
export async function indexVenueApproval(db: Db, space: string, rkey: string, value: JsonObject): Promise<boolean> {
	if (!isValidRecord<VenueApprovalRecord>(COLLECTION.venueApproval, value)) return false;
	const row = { eventUri: value.event, approvedBy: value.approvedBy, createdAt: value.createdAt };

	await db
		.insertInto('venueApproval')
		.values({ spaceUri: space, rkey, ...row })
		.onConflict((oc) => oc.columns(['spaceUri', 'rkey']).doUpdateSet(row))
		.execute();

	return true;
}
