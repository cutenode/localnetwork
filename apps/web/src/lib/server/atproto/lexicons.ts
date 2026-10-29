import type { Agent } from '@atproto/api';
import { isObject } from '@atproto/lex-data';
import type { JsonObject, JsonValue } from '@atproto/lex-json';
import { jsonToLex, type LexiconDoc, lexPermissionSet, Lexicons, parseLexiconDoc } from '@atproto/lexicon';
import { LEXICON_FILES } from '#lexicon-files';
import { NS, PERMISSIONS } from '#lib/nsid.ts';

/** Every document in lexicons/, by NSID, so the order doesn't depend on which loader read them. */
const DOCS: LexiconDoc[] = LEXICON_FILES.map((file) => parseLexiconDoc(file)).toSorted((a, b) =>
	a.id.localeCompare(b.id)
);

/** Ours are every Local Network tool's, all of lexicons/computer/; the other folders are other authorities'. */
const isOurs = (doc: LexiconDoc) => doc.id.startsWith(`${NS}.`);

/** Every Local Network lexicon, for validation and publishing. */
export const LEXICON_DOCS = DOCS.filter(isOurs);

/** The permission set as PDSes read it to expand `include:computer.localnetwork.permissions` at sign-in. */
export const PERMISSION_SET = lexPermissionSet.parse(LEXICON_DOCS.find((doc) => doc.id === PERMISSIONS)?.defs.main);

/**
 * Other authorities' lexicons that ours reference, this app writes, or HappyView keeps for it (calendar events and
 * the locations they can hold, strongRef, Bluesky's verifications, profiles and follows and the label defs profiles
 * refer to, and userinput.app's spaces and the bug reports posted to them), copied into lexicons/ as they publish
 * them. They're validated against and offered to HappyView, never published.
 */
export const REFERENCED_DOCS = DOCS.filter((doc) => !isOurs(doc));

// Lexicons rewrites refs in place (`#claim` becomes `lex:…#claim`), so it gets a copy and the exported docs stay publishable.
export const lexicons = new Lexicons(structuredClone(DOCS));

/** Publishes every lexicon as a com.atproto.lexicon.schema record in the agent's own repo. */
export async function publishLexicons(agent: Agent): Promise<void> {
	for (const doc of LEXICON_DOCS)
		await agent.com.atproto.repo.putRecord({
			repo: agent.assertDid,
			collection: 'com.atproto.lexicon.schema',
			rkey: doc.id,
			record: { $type: 'com.atproto.lexicon.schema', ...JSON.parse(JSON.stringify(doc)) }
		});
}

export class RecordValidationError extends Error {
	constructor(
		readonly collection: string,
		message: string
	) {
		super(`Invalid ${collection} record: ${message}`);
		this.name = 'RecordValidationError';
	}
}

/** Whether `value` holds a legacy `{ cid, mimeType }` blob ref anywhere. */
function hasLegacyBlob(value: JsonValue | undefined): boolean {
	if (Array.isArray(value)) return value.some(hasLegacyBlob);

	if (!isObject(value)) return false;

	return ('cid' in value && 'mimeType' in value && !('$type' in value)) || Object.values(value).some(hasLegacyBlob);
}

/**
 * Validates a record against its lexicon before it is written anywhere, and
 * returns it (still in JSON form, e.g. `{ $link }` blob refs) with `$type` set.
 * Validation runs on the lex form so blob refs and CIDs are checked as such.
 * Legacy blob refs pass lexicon validation, but every record type here assumes
 * the typed `{ $type: 'blob', ref }` form, so they're rejected too.
 */
export function validateRecord<T extends JsonObject>(collection: string, record: T): T & { $type: string } {
	const typed = { ...record, $type: collection };

	if (hasLegacyBlob(typed)) throw new RecordValidationError(collection, 'legacy blob refs are not supported');

	try {
		lexicons.assertValidRecord(collection, jsonToLex(typed));
	} catch (error) {
		throw new RecordValidationError(collection, error instanceof Error ? error.message : String(error));
	}

	return typed;
}
