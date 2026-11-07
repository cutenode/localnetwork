import { grantVia } from '#lib/epk.ts';
import { epkGrantSchema } from '#lib/epk-schema.ts';
import { COLLECTION, SPACE_TYPE } from '#lib/nsid.ts';
import { listAllSpaceRecords, memberSpacePolicy } from '../atproto/repo-client';
import { parseSpaceRecordUri, parseSpaceUri } from '../atproto/spaces';
import type { Core } from '../core';
import { deleteAt, type EpkService, type PressKit } from './epk';

/**
 * Moves the grants and requests out of the root's press kit spaces, where everyone granted a press kit could read them,
 * into each project's request space, which only its managers read. Runs at startup, after the sync has indexed every
 * request and grant, so a move that stopped part way finishes on a later start.
 *
 * Grants go first. The root wrote them all, so each is copied as the root and the old record deleted; a space with no
 * grant records left is done. Then each request is copied as the root on its asker's behalf with its status, and the
 * old record is deleted: by the root when it wrote it, else with the asker's own session, so one whose asker isn't
 * signed in, or whose session can't delete it, waits. Then grants that named it are rewritten to name the copy. Once a
 * space holds no requests, it stops taking public writes, which marks its requests moved.
 */
export async function moveKitSpaces(core: Core, epks: EpkService): Promise<void> {
	const root = await core.root();

	for (const space of await root.listSpaces(SPACE_TYPE.epk)) {
		// A legacy press kit's space, which the root writes in too, moves with the press kit on its maker's visit.
		if (parseSpaceUri(space)?.authority !== root.did) continue;

		try {
			await moveGrants(core, epks, space);
			const policy = (await root.getSpace(space))?.policy;

			if (!policy || !('publicWrite' in policy) || !policy.publicWrite) continue;
			await moveRequests(core, epks, space);
		} catch (error) {
			core.logger.warn("could not move a press kit's grants or requests; the next start tries again", {
				err: error,
				space
			});
		}
	}
}

/** The root press kit whose space `space` is, or null, logged, when it's gone. */
async function kitOf(core: Core, epks: EpkService, space: string): Promise<PressKit | null> {
	const kit = await epks.locate(core.rootDid, parseSpaceUri(space)!.skey);

	if (kit?.epk.space === space) return kit;
	core.logger.warn('leaving the grants and requests of a press kit space with no press kit', { space });

	return null;
}

async function moveGrants(core: Core, epks: EpkService, space: string): Promise<void> {
	const root = await core.root();
	const records = await listAllSpaceRecords(root, { space, repo: root.did, collection: COLLECTION.epkGrant });

	if (records.length === 0) return;
	const kit = await kitOf(core, epks, space);

	if (!kit) return;

	let moved = 0;

	for (const record of records) {
		const grant = epkGrantSchema.safeParse(record.value);

		if (!grant.success) {
			core.logger.warn('leaving a grant record that does not validate', { space, rkey: record.rkey });
			continue;
		}

		const { subject, via, request, createdAt } = grant.data;
		await epks.writeGrant(kit.project, kit.epk, { subject, via: grantVia(via), request, createdAt });
		await root.deleteSpaceRecord({ space, collection: COLLECTION.epkGrant, rkey: record.rkey });
		moved++;
	}

	if (moved === 0) return;
	core.logger.info("moved a press kit's grants to its project's request space", { space, moved });
	await core.audit.record({
		actor: root.did,
		actorRole: 'system',
		action: 'system.space',
		subject: space,
		subjectKind: 'space',
		summary: `Moved the grants to see the press kit “${kit.epk.record.title}” to a space only its managers read`,
		via: 'system',
		details: { moved, project: kit.project.uri }
	});
}

async function moveRequests(core: Core, epks: EpkService, space: string): Promise<void> {
	const root = await core.root();
	const kit = await kitOf(core, epks, space);

	if (!kit) return;

	const rows = await core.db.selectFrom('epkRequest').selectAll().where('spaceUri', '=', space).execute();
	let left = 0;

	for (const row of rows) {
		const old = parseSpaceRecordUri(row.uri)!;
		const writer = old.author === root.did ? root : await core.userClient(old.author);

		if (!writer) {
			core.logger.info('a request stays in its press kit space until its asker signs in again', { uri: row.uri });
			left++;
			continue;
		}

		const copy = await epks.copyRequest(row, kit.project, kit.epk.uri);

		try {
			await writer.deleteSpaceRecord({ space, collection: COLLECTION.epkRequest, rkey: old.rkey });
		} catch (error) {
			// A request moves whole or not at all, so the managers never see it twice.
			await deleteAt(root, copy);
			await core.db.deleteFrom('epkRequest').where('uri', '=', copy).execute();
			core.logger.warn('could not delete a request from its press kit space', { err: error, uri: row.uri });
			left++;
			continue;
		}

		const grants = await core.db.selectFrom('epkGrant').selectAll().where('requestUri', '=', row.uri).execute();

		for (const grant of grants) {
			const via = grantVia(grant.via);
			await epks.writeGrant(kit.project, kit.epk, {
				subject: grant.subjectDid,
				via,
				request: copy,
				createdAt: grant.createdAt
			});
		}

		await core.db.deleteFrom('epkRequest').where('uri', '=', row.uri).execute();
	}

	if (left > 0) return;
	await root.updateSpace({ space, policy: memberSpacePolicy() });
	core.logger.info("moved a press kit's requests to its project's request space", { space, moved: rows.length });
	await core.audit.record({
		actor: root.did,
		actorRole: 'system',
		action: 'system.space',
		subject: space,
		subjectKind: 'space',
		summary: `Moved the requests to see the press kit “${kit.epk.record.title}” to a space only its managers read`,
		via: 'system',
		details: { moved: rows.length, project: kit.project.uri }
	});
}
