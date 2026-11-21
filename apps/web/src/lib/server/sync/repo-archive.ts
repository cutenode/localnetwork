import { ComAtprotoSpaceDefs } from '@atproto/api';
import { decode } from '@atproto/lex-cbor';
import { type Cid, cidForCbor, isPlainObject } from '@atproto/lex-data';
import { type JsonObject, lexToJson } from '@atproto/lex-json';
import {
	MAX_RECORD_SIZE,
	parseRecordPath,
	type RecordPathParts,
	RepoCommit,
	type SpaceCommit,
	SpaceRepoVerificationError,
	verifyCommit
} from '@atproto/space';
import { readStar } from '@atproto/star';
import { isTooDeep } from '../atproto/lexicons';

/** Whether `value` is a `com.atproto.space.defs#commit` in its lex form, as a commit is checked against its MAC. */
export const isSpaceCommit = (value: unknown): value is SpaceCommit =>
	ComAtprotoSpaceDefs.validateCommit(value).success;

/** What a repo archive's metadata names its profile with. */
const REPO_STAR_TYPE = 'atproto-space-repo';

/** The frame limits @atproto/space reads a repo archive with (its REPO_STAR_LIMITS, which it doesn't export). */
const LIMITS = { maxMetaLength: 64 * 1024, maxKeyLength: 4 * 1024, maxValueLength: MAX_RECORD_SIZE };

/** A record from a repo archive: its value as JSON, or null when it's nested deeper than validateRecord reads. */
export type ArchivedRecord = RecordPathParts & { cid: Cid; value: JsonObject | null };

/**
 * Reads a `getRepo` archive as @atproto/space's verifyRepoStarFull does: the commit checked against its MAC, then
 * every record, which must fold into the commit's data. The difference is a record nested too deep to read. The
 * CBOR decoder and lexToJson recurse, so one a few thousand levels deep overflows the stack, and verifyRepoStarFull
 * fails the whole repo for it. Here it comes back with a null value instead, and still counts toward the commit's
 * data, since its CID is the hash of its bytes, which needs no decoding.
 */
export async function readRepoArchive(
	body: AsyncIterable<Uint8Array>,
	params: Parameters<typeof verifyCommit>[1]
): Promise<{ commit: SpaceCommit; records: ArchivedRecord[]; state: RepoCommit }> {
	const archive = await readStar(body, LIMITS);

	try {
		const { $type, ...commit } = archive.meta;

		if ($type !== REPO_STAR_TYPE || !isSpaceCommit(commit)) {
			throw new SpaceRepoVerificationError(`invalid ${REPO_STAR_TYPE} metadata`);
		}

		verifyCommit(commit, params);

		const records: ArchivedRecord[] = [];
		const decoder = new TextDecoder();
		let previous: Uint8Array | undefined;

		for await (const { key, value } of archive.pairs) {
			const path = decoder.decode(key);
			const { collection, rkey } = parseRecordPath(path);

			if (previous && Buffer.compare(key, previous) <= 0) {
				throw new SpaceRepoVerificationError(`records out of order at ${path}`);
			}

			previous = key;
			records.push({ collection, rkey, cid: await cidForCbor(value), value: readValue(value, path) });
		}

		const state = RepoCommit.fromRecords(records);

		if (!state.matches(commit)) throw new SpaceRepoVerificationError('records do not match the commit data');

		return { commit, records, state };
	} finally {
		await archive[Symbol.asyncDispose]();
	}
}

/** A record's DRISL bytes as JSON, or null when it's nested deeper than validateRecord reads. */
function readValue(bytes: Uint8Array, path: string): JsonObject | null {
	try {
		const value = decode(bytes);

		if (!isPlainObject(value)) throw new SpaceRepoVerificationError(`invalid record at ${path}`);

		// SAFETY: lexToJson only rewrites the CIDs and bytes inside a map, so a map always comes back as a JSON object.
		const json = lexToJson(value) as JsonObject;

		return isTooDeep(json) ? null : json;
	} catch (cause) {
		// The decoder reports malformed bytes with a plain Error, so a RangeError is the stack running out.
		if (cause instanceof RangeError) return null;

		if (cause instanceof SpaceRepoVerificationError) throw cause;
		throw new SpaceRepoVerificationError(`invalid record cbor at ${path}`, { cause });
	}
}
