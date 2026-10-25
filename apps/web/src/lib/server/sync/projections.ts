import type { JsonObject } from '@atproto/lex-json';
import { COLLECTION, SPACE_TYPE } from '#lib/nsid.ts';
import type { ProjectDetails, RequestRecord } from '#lib/types.ts';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import { parseSpaceUri } from '../atproto/spaces';
import type { AuditLog } from '../audit/log';
import type { Core } from '../core';
import type { BookingRecord, BookingService } from '../services/bookings';
import type { EpkService } from '../services/epk';
import type { ProjectService } from '../services/projects';
import { isRequestKind, type RequestService } from '../services/requests';
import { type StaffService, staffRoleRecord } from '../services/staff';
import type { ChangeHandler } from './engine';

/**
 * Maps records synced from spaces onto the local index. Records are checked
 * against our lexicons, and authority rules are enforced here: only the root
 * account's decisions, roles, and details count, and only the grants of a press
 * kit space's authority.
 */
export function createProjections(deps: {
	core: Core;
	audit: AuditLog;
	projects: ProjectService;
	requests: RequestService;
	staff: StaffService;
	bookings: BookingService;
	epks: EpkService;
}): ChangeHandler {
	const { core, audit, projects, requests, staff, bookings, epks } = deps;
	const { spaces, rootDid } = core;

	return async (change) => {
		const fromRoot = change.repoDid === rootDid;
		const type = parseSpaceUri(change.space)?.type;

		if (change.kind === 'delete') {
			if (change.collection === COLLECTION.staffRole && change.space === spaces.staff && fromRoot) {
				await staff.removeRoleByUri(change.uri);
			} else if (change.collection === COLLECTION.request && change.space === spaces.moderation) {
				await core.db
					.updateTable('request')
					.set({ status: 'withdrawn' })
					.where('uri', '=', change.uri)
					.where('status', 'in', ['pending', 'changes_requested'])
					.execute();
			} else if (change.collection === COLLECTION.auditEntry && change.space === spaces.audit) {
				await core.db.deleteFrom('auditCopy').where('uri', '=', change.uri).execute();
				const column = fromRoot ? 'rootUri' : 'actorUri';
				await core.db
					.updateTable('auditEntry')
					.set(
						fromRoot
							? { rootStatus: 'failed', rootError: 'Copy was deleted from the repo' }
							: { actorStatus: 'failed', actorError: 'Copy was deleted from the repo' }
					)
					.where(column, '=', change.uri)
					.execute();
				core.logger.warn({ uri: change.uri }, 'an audit copy was deleted');
			} else if (type === SPACE_TYPE.booking) {
				await core.db.deleteFrom('bookingEvent').where('uri', '=', change.uri).execute();
			} else if (type === SPACE_TYPE.epk && change.collection === COLLECTION.epkGrant) {
				await epks.unindexGrant(change.space, change.uri);
			} else if (type === SPACE_TYPE.epk && change.collection === COLLECTION.epkRequest) {
				await epks.unindexRequest(change.uri);
			} else if (type === SPACE_TYPE.backstage && change.collection === COLLECTION.epkDraft) {
				await epks.drafts.unindex(change.uri);
			}

			return;
		}

		let value: JsonObject;

		try {
			value = validateRecord(change.collection, change.value);
		} catch (error) {
			if (!(error instanceof RecordValidationError)) throw error;
			core.logger.warn({ uri: change.uri, err: error }, 'ignoring invalid synced record');

			return;
		}

		switch (change.collection) {
			case COLLECTION.auditEntry:
				if (change.space === spaces.audit) {
					await audit.onSyncedCopy({
						uri: change.uri,
						repoDid: change.repoDid,
						rkey: change.rkey,
						cid: change.cid,
						value
					});
				}

				return;
			case COLLECTION.request:
				if (change.space !== spaces.moderation) return;

				if (!isRequestKind(value.kind)) {
					core.logger.warn({ uri: change.uri, kind: value.kind }, 'ignoring synced request of an unknown kind');

					return;
				}

				await requests.index({
					uri: change.uri,
					cid: change.cid,
					repoDid: change.repoDid,
					// SAFETY: validateRecord checked `value` against the request lexicon, which RequestRecord mirrors, and
					// `kind`, the one field RequestRecord narrows further, was just checked against RequestKind.
					value: value as RequestRecord
				});

				return;
			case COLLECTION.decision:
				if (change.space === spaces.moderation && fromRoot) await requests.applyDecision({ uri: change.uri, value });

				return;
			case COLLECTION.staffRole: {
				if (change.space !== spaces.staff || !fromRoot) return;
				const role = staffRoleRecord.safeParse(value);

				if (role.success) {
					await staff.indexRole(change.uri, change.cid, role.data);

					return;
				}

				// The record no longer holds a role this app knows, so it grants nothing, including what it held before.
				core.logger.warn({ uri: change.uri, role: value.role }, 'ignoring synced staff role this app does not know');
				await staff.removeRoleByUri(change.uri);

				return;
			}

			case COLLECTION.projectDetails:
				if (change.space === spaces.industry && fromRoot) {
					// SAFETY: validateRecord checked `value` against the project details lexicon, which requires these three
					// fields, and ProjectDetails mirrors its `details` object (every field an optional string).
					const record = value as { project: string; details: ProjectDetails; updatedAt: string };
					await projects.indexDetails(record.project, change.uri, change.cid, record.details, record.updatedAt);
				}

				return;
			case COLLECTION.bookingInquiry:
			case COLLECTION.bookingMessage:
			case COLLECTION.bookingStatus:
				if (type === SPACE_TYPE.booking && parseSpaceUri(change.space)?.authority === rootDid) {
					await bookings.indexEvent({
						space: change.space,
						uri: change.uri,
						repoDid: change.repoDid,
						collection: change.collection,
						// SAFETY: validateRecord checked `value` against this booking collection's lexicon, and each of the three
						// is a subset of BookingRecord with `createdAt` required.
						value: value as BookingRecord
					});
				}

				return;
			case COLLECTION.epkRequest:
				if (type === SPACE_TYPE.epk)
					await epks.indexRequest({ space: change.space, uri: change.uri, repoDid: change.repoDid, value });

				return;
			case COLLECTION.epkGrant:
				if (type === SPACE_TYPE.epk)
					await epks.indexGrant({ space: change.space, uri: change.uri, repoDid: change.repoDid, value });

				return;
			case COLLECTION.epkDraft:
				if (type === SPACE_TYPE.backstage) {
					await epks.drafts.index({
						space: change.space,
						uri: change.uri,
						repoDid: change.repoDid,
						rkey: change.rkey,
						cid: change.cid,
						value
					});
				}

				return;
			default:
				return;
		}
	};
}
