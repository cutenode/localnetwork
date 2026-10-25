import type { Agent } from '@atproto/api';
import { isObject } from '@atproto/lex-data';
import type { JsonObject, JsonValue } from '@atproto/lex-json';
import { jsonToLex, lexPermissionSet, Lexicons, parseLexiconDoc } from '@atproto/lexicon';
import auditEntry from '../../../../../../lexicons/computer/localnetwork/audit/entry.json' with { type: 'json' };
import bookingInquiry from '../../../../../../lexicons/computer/localnetwork/booking/inquiry.json' with { type: 'json' };
import bookingMessage from '../../../../../../lexicons/computer/localnetwork/booking/message.json' with { type: 'json' };
import bookingStatus from '../../../../../../lexicons/computer/localnetwork/booking/status.json' with { type: 'json' };
import calendarEvent from '../../../../../../lexicons/community/lexicon/calendar/event.json' with { type: 'json' };
import address from '../../../../../../lexicons/community/lexicon/location/address.json' with { type: 'json' };
import fsq from '../../../../../../lexicons/community/lexicon/location/fsq.json' with { type: 'json' };
import geo from '../../../../../../lexicons/community/lexicon/location/geo.json' with { type: 'json' };
import hthree from '../../../../../../lexicons/community/lexicon/location/hthree.json' with { type: 'json' };
import strongRef from '../../../../../../lexicons/com/atproto/repo/strongRef.json' with { type: 'json' };
import discussion from '../../../../../../lexicons/app/userinput/discussion.json' with { type: 'json' };
import userinputSpace from '../../../../../../lexicons/app/userinput/space.json' with { type: 'json' };
import bskyProfile from '../../../../../../lexicons/app/bsky/actor/profile.json' with { type: 'json' };
import bskyFollow from '../../../../../../lexicons/app/bsky/graph/follow.json' with { type: 'json' };
import verification from '../../../../../../lexicons/app/bsky/graph/verification.json' with { type: 'json' };
import labelDefs from '../../../../../../lexicons/com/atproto/label/defs.json' with { type: 'json' };
import defs from '../../../../../../lexicons/computer/localnetwork/defs.json' with { type: 'json' };
import directoryDefs from '../../../../../../lexicons/computer/localnetwork/directory/defs.json' with { type: 'json' };
import epk from '../../../../../../lexicons/computer/localnetwork/epk.json' with { type: 'json' };
import epkContent from '../../../../../../lexicons/computer/localnetwork/epk/content.json' with { type: 'json' };
import epkDefs from '../../../../../../lexicons/computer/localnetwork/epk/defs.json' with { type: 'json' };
import epkGrant from '../../../../../../lexicons/computer/localnetwork/epk/grant.json' with { type: 'json' };
import epkRequest from '../../../../../../lexicons/computer/localnetwork/epk/request.json' with { type: 'json' };
import follow from '../../../../../../lexicons/computer/localnetwork/follow.json' with { type: 'json' };
import locallibrarySettings from '../../../../../../lexicons/computer/localnetwork/locallibrary/settings.json' with { type: 'json' };
import getArtist from '../../../../../../lexicons/computer/localnetwork/directory/getArtist.json' with { type: 'json' };
import getCharts from '../../../../../../lexicons/computer/localnetwork/directory/getCharts.json' with { type: 'json' };
import getFollowStatus from '../../../../../../lexicons/computer/localnetwork/directory/getFollowStatus.json' with { type: 'json' };
import getProject from '../../../../../../lexicons/computer/localnetwork/directory/getProject.json' with { type: 'json' };
import getProjects from '../../../../../../lexicons/computer/localnetwork/directory/getProjects.json' with { type: 'json' };
import getRecords from '../../../../../../lexicons/computer/localnetwork/directory/getRecords.json' with { type: 'json' };
import getSets from '../../../../../../lexicons/computer/localnetwork/directory/getSets.json' with { type: 'json' };
import getShows from '../../../../../../lexicons/computer/localnetwork/directory/getShows.json' with { type: 'json' };
import getStats from '../../../../../../lexicons/computer/localnetwork/directory/getStats.json' with { type: 'json' };
import listSets from '../../../../../../lexicons/computer/localnetwork/directory/listSets.json' with { type: 'json' };
import listRecords from '../../../../../../lexicons/computer/localnetwork/directory/listRecords.json' with { type: 'json' };
import listShows from '../../../../../../lexicons/computer/localnetwork/directory/listShows.json' with { type: 'json' };
import requestIndex from '../../../../../../lexicons/computer/localnetwork/directory/requestIndex.json' with { type: 'json' };
import djDefs from '../../../../../../lexicons/computer/localnetwork/dj/defs.json' with { type: 'json' };
import djProfile from '../../../../../../lexicons/computer/localnetwork/dj/profile.json' with { type: 'json' };
import djSet from '../../../../../../lexicons/computer/localnetwork/dj/set.json' with { type: 'json' };
import matchProjects from '../../../../../../lexicons/computer/localnetwork/directory/matchProjects.json' with { type: 'json' };
import searchArtists from '../../../../../../lexicons/computer/localnetwork/directory/searchArtists.json' with { type: 'json' };
import searchProjects from '../../../../../../lexicons/computer/localnetwork/directory/searchProjects.json' with { type: 'json' };
import decision from '../../../../../../lexicons/computer/localnetwork/moderation/decision.json' with { type: 'json' };
import request from '../../../../../../lexicons/computer/localnetwork/moderation/request.json' with { type: 'json' };
import show from '../../../../../../lexicons/computer/localnetwork/show.json' with { type: 'json' };
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
	listShows,
	getShows,
	getFollowStatus,
	getCharts,
	getArtist,
	searchArtists,
	getRecords,
	listRecords,
	requestIndex,
	djDefs,
	djProfile,
	djSet,
	show,
	follow,
	locallibrarySettings
].map((doc) => parseLexiconDoc(doc));

/** The permission set as PDSes read it to expand `include:computer.localnetwork.permissions` at sign-in. */
export const PERMISSION_SET = lexPermissionSet.parse(permissions.defs.main);

/**
 * Other authorities' lexicons that ours reference, this app writes, or HappyView keeps for it (calendar events, the
 * locations they can hold, strongRef, Bluesky's verifications, profiles and follows and the label defs profiles refer
 * to, and userinput.app's space), copied into lexicons/ as they publish them.
 */
export const REFERENCED_DOCS = [
	address,
	geo,
	fsq,
	hthree,
	calendarEvent,
	strongRef,
	labelDefs,
	verification,
	bskyProfile,
	bskyFollow,
	userinputSpace
].map((doc) => parseLexiconDoc(doc));

/** Other apps' records this app writes but nothing indexes: bug reports on userinput.app. Only validated, never published. */
const WRITTEN_DOCS = [parseLexiconDoc(discussion)];

// Lexicons rewrites refs in place (`#claim` becomes `lex:…#claim`), so it gets a copy and the exported docs stay publishable.
export const lexicons = new Lexicons(structuredClone([...LEXICON_DOCS, ...REFERENCED_DOCS, ...WRITTEN_DOCS]));

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
