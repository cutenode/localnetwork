import { type Agent, schemas } from '@atproto/api';
import { isObject } from '@atproto/lex-data';
import type { JsonObject, JsonValue } from '@atproto/lex-json';
import { jsonToLex, lexPermissionSet, type LexiconDoc, Lexicons, parseLexiconDoc } from '@atproto/lexicon';
import { LexResolver } from '@atproto/lex-resolver';
import auditEntry from '../../../../../../lexicons/computer/localnetwork/audit/entry.json' with { type: 'json' };
import bookingInquiry from '../../../../../../lexicons/computer/localnetwork/booking/inquiry.json' with { type: 'json' };
import bookingMessage from '../../../../../../lexicons/computer/localnetwork/booking/message.json' with { type: 'json' };
import bookingStatus from '../../../../../../lexicons/computer/localnetwork/booking/status.json' with { type: 'json' };
import discussion from '../../../../../../lexicons/app/userinput/discussion.json' with { type: 'json' };
import defs from '../../../../../../lexicons/computer/localnetwork/defs.json' with { type: 'json' };
import directoryDefs from '../../../../../../lexicons/computer/localnetwork/directory/defs.json' with { type: 'json' };
import epk from '../../../../../../lexicons/computer/localnetwork/epk.json' with { type: 'json' };
import epkContent from '../../../../../../lexicons/computer/localnetwork/epk/content.json' with { type: 'json' };
import epkDefs from '../../../../../../lexicons/computer/localnetwork/epk/defs.json' with { type: 'json' };
import epkGrant from '../../../../../../lexicons/computer/localnetwork/epk/grant.json' with { type: 'json' };
import epkRequest from '../../../../../../lexicons/computer/localnetwork/epk/request.json' with { type: 'json' };
import getArtist from '../../../../../../lexicons/computer/localnetwork/directory/getArtist.json' with { type: 'json' };
import getCharts from '../../../../../../lexicons/computer/localnetwork/directory/getCharts.json' with { type: 'json' };
import getProject from '../../../../../../lexicons/computer/localnetwork/directory/getProject.json' with { type: 'json' };
import getProjects from '../../../../../../lexicons/computer/localnetwork/directory/getProjects.json' with { type: 'json' };
import getSets from '../../../../../../lexicons/computer/localnetwork/directory/getSets.json' with { type: 'json' };
import getStats from '../../../../../../lexicons/computer/localnetwork/directory/getStats.json' with { type: 'json' };
import listSets from '../../../../../../lexicons/computer/localnetwork/directory/listSets.json' with { type: 'json' };
import djDefs from '../../../../../../lexicons/computer/localnetwork/dj/defs.json' with { type: 'json' };
import djSet from '../../../../../../lexicons/computer/localnetwork/dj/set.json' with { type: 'json' };
import matchProjects from '../../../../../../lexicons/computer/localnetwork/directory/matchProjects.json' with { type: 'json' };
import searchArtists from '../../../../../../lexicons/computer/localnetwork/directory/searchArtists.json' with { type: 'json' };
import searchProjects from '../../../../../../lexicons/computer/localnetwork/directory/searchProjects.json' with { type: 'json' };
import decision from '../../../../../../lexicons/computer/localnetwork/moderation/decision.json' with { type: 'json' };
import request from '../../../../../../lexicons/computer/localnetwork/moderation/request.json' with { type: 'json' };
import permissions from '../../../../../../lexicons/computer/localnetwork/permissions.json' with { type: 'json' };
import plannerSave from '../../../../../../lexicons/computer/localnetwork/planner/save.json' with { type: 'json' };
import plannerShow from '../../../../../../lexicons/computer/localnetwork/planner/show.json' with { type: 'json' };
import project from '../../../../../../lexicons/computer/localnetwork/project.json' with { type: 'json' };
import projectDetails from '../../../../../../lexicons/computer/localnetwork/project/details.json' with { type: 'json' };
import spaceAudit from '../../../../../../lexicons/computer/localnetwork/space/audit.json' with { type: 'json' };
import spaceBooking from '../../../../../../lexicons/computer/localnetwork/space/booking.json' with { type: 'json' };
import spaceEpk from '../../../../../../lexicons/computer/localnetwork/space/epk.json' with { type: 'json' };
import spaceIndustry from '../../../../../../lexicons/computer/localnetwork/space/industry.json' with { type: 'json' };
import spaceModeration from '../../../../../../lexicons/computer/localnetwork/space/moderation.json' with { type: 'json' };
import spacePlanner from '../../../../../../lexicons/computer/localnetwork/space/planner.json' with { type: 'json' };
import spaceStaff from '../../../../../../lexicons/computer/localnetwork/space/staff.json' with { type: 'json' };
import staffRole from '../../../../../../lexicons/computer/localnetwork/staff/role.json' with { type: 'json' };

/** Every Local Network lexicon this app uses, for validation and publishing. */
export const LEXICON_DOCS = [
	defs,
	project,
	projectDetails,
	auditEntry,
	request,
	decision,
	staffRole,
	bookingInquiry,
	bookingMessage,
	bookingStatus,
	plannerSave,
	plannerShow,
	epkDefs,
	epk,
	epkContent,
	epkRequest,
	epkGrant,
	spaceAudit,
	spaceModeration,
	spaceStaff,
	spaceIndustry,
	spaceBooking,
	spacePlanner,
	spaceEpk,
	permissions,
	directoryDefs,
	searchProjects,
	matchProjects,
	getProject,
	getProjects,
	getStats,
	getSets,
	listSets,
	getCharts,
	getArtist,
	searchArtists,
	djDefs,
	djSet
].map((doc) => parseLexiconDoc(doc));

/** The permission set as PDSes read it to expand `include:computer.localnetwork.permissions` at sign-in. */
export const PERMISSION_SET = lexPermissionSet.parse(permissions.defs.main);

/** Other authorities' lexicons that ours reference, which they publish: fetched as published (see loadReferencedLexicons). */
export const REFERENCED_NSIDS = ['community.lexicon.location.address'];

/** strongRef, as the AT Protocol's own client library ships it. */
const STRONG_REF = schemas.filter((doc) => doc.id === 'com.atproto.repo.strongRef').map((doc) => parseLexiconDoc(doc));

/** Other apps' records this app writes for its users: bug reports on userinput.app. Only validated, never published or indexed. */
const WRITTEN_DOCS = [discussion].map((doc) => parseLexiconDoc(doc));

// Lexicons rewrites refs in place (`#claim` becomes `lex:…#claim`), so it gets a copy and the exported docs stay publishable.
export const lexicons = new Lexicons(structuredClone([...LEXICON_DOCS, ...STRONG_REF, ...WRITTEN_DOCS]));

/** Fetches one lexicon as its authority publishes it. */
export type LexiconSource = (nsid: string) => Promise<LexiconDoc>;

/** Its `_lexicon` DNS record names the authority's DID, and the lexicon is a record in that DID's repo. */
const live: LexiconSource = async (nsid) => parseLexiconDoc((await new LexResolver({}).get(nsid)).lexicon);

let referenced: Promise<LexiconDoc[]> | undefined;

let loaded: LexiconDoc[] | undefined;

async function load(source: LexiconSource): Promise<LexiconDoc[]> {
	const docs = await Promise.all(REFERENCED_NSIDS.map((nsid) => source(nsid)));

	for (const doc of docs) lexicons.add(structuredClone(doc));

	loaded = [...docs, ...STRONG_REF];

	return loaded;
}

/**
 * The other authorities' lexicons ours reference (REFERENCED_NSIDS) with strongRef, fetched from `source` once per
 * process and added to `lexicons`, so records that use them validate. A failed fetch is tried again on the next call.
 */
export function loadReferencedLexicons(source: LexiconSource = live): Promise<LexiconDoc[]> {
	if (referenced) return referenced;

	const loading = load(source);
	referenced = loading;
	loading.catch(() => {
		if (referenced === loading) referenced = undefined;
	});

	return loading;
}

/** What loadReferencedLexicons fetched, which has to have finished first (the app waits for it as it starts). */
export function referencedLexicons(): LexiconDoc[] {
	if (!loaded) throw new Error('The lexicons ours reference are not loaded yet: await loadReferencedLexicons() first');

	return loaded;
}

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
