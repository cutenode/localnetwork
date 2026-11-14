import type { JsonObject } from '@atproto/lex-json';
import { z } from 'zod';
import { COLLECTION } from '#lib/nsid.ts';
import type { PerformerRecord } from '#lib/types.ts';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import { recordCid } from '../atproto/repo-client';
import { parseSpaceUri } from '../atproto/spaces';
import { type Db, parseJson } from '../db';
import type { ListingSource } from './listings';

/** The performer record's one key in each performer space. */
export const PERFORMER_KEY = 'self';

export const ACT_TYPE = `${COLLECTION.performer}#act` as const;

export const DJ_TYPE = `${COLLECTION.performer}#dj` as const;

/** The subjects this app knows, of the performer record's open union. */
const knownSubject = z.object({ subject: z.object({ $type: z.enum([ACT_TYPE, DJ_TYPE]) }) });

/**
 * Whether `value` is a performer record this app reads: valid by the performer lexicon, which PerformerRecord mirrors,
 * with a subject of a kind it knows rather than one a newer app added.
 */
function isPerformerRecord(value: JsonObject): value is JsonObject & PerformerRecord {
	try {
		validateRecord(COLLECTION.performer, value);
	} catch (error) {
		if (error instanceof RecordValidationError) return false;
		throw error;
	}

	return knownSubject.safeParse(value).success;
}

/** `value` as a performer record this app reads, or null for one it can't. */
export function performerRecord(value: JsonObject | undefined): PerformerRecord | null {
	return value && isPerformerRecord(value) ? value : null;
}

/**
 * Indexes the root's performer record in a performer space, or returns false when it isn't one this app reads.
 * Callers check the root wrote it, in a performer space under the root, at its key.
 */
export async function indexPerformer(db: Db, space: string, value: JsonObject): Promise<boolean> {
	const record = performerRecord(value);
	const skey = parseSpaceUri(space)?.skey;

	if (!record || !skey) return false;
	const { subject } = record;
	const isAct = subject.$type === ACT_TYPE;

	const row = {
		skey,
		kind: isAct ? ('act' as const) : ('dj' as const),
		slug: isAct ? subject.slug : null,
		did: isAct ? null : subject.did,
		record: JSON.stringify(record),
		formerly: JSON.stringify(record.formerly ?? []),
		cid: await recordCid({ ...value, $type: COLLECTION.performer }),
		updatedAt: record.updatedAt
	};

	await db
		.insertInto('performer')
		.values({ spaceUri: space, ...row })
		.onConflict((oc) => oc.column('spaceUri').doUpdateSet(row))
		.execute();

	return true;
}

/**
 * What a listed act's listing shows: its name, slug and public sections. A DJ's own space is never listed as an act,
 * since the DJ's profile is their page. The avatar is a blob the root uploaded when the page was saved, so the root's
 * public listing can hold it.
 */
export function performerListingSource(db: Db): ListingSource {
	return {
		async publicFields(space) {
			const row = await db.selectFrom('performer').select('record').where('spaceUri', '=', space).executeTakeFirst();
			const subject = parseJson<PerformerRecord | null>(row?.record, null)?.subject;

			if (subject?.$type !== ACT_TYPE) return null;
			const { slug, form, basics, performer } = subject;
			const fields: JsonObject = JSON.parse(JSON.stringify({ form, basics, performer }));

			return { name: basics.name, slug, fields };
		}
	};
}
