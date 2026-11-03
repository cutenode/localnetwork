import { COLLECTION, SPACE_TYPE } from '#lib/nsid.ts';
import { memberSpacePolicy } from '../atproto/repo-client';
import { parseSpaceRecordUri, parseSpaceUri, spaceOfRecord } from '../atproto/spaces';
import type { Core } from '../core';
import type { EpkService } from './epk';

/**
 * Moves requests out of the root's press kit spaces, where everyone granted a press kit could read them, into each
 * project's request space, which only its managers read. Each request is copied as the root on its asker's behalf with
 * its status, and the old record is deleted: by the root when it wrote it, else with the asker's own session, so one
 * whose asker isn't signed in, or whose session can't delete it, waits. Then grants that named it are rewritten to name the copy. Once a space
 * holds no requests, it stops taking public writes, which marks it moved. Runs at startup, after the sync has indexed
 * every request, so a move that stopped part way finishes on a later start.
 */
export async function moveKitRequests(core: Core, epks: EpkService): Promise<void> {
	const root = await core.root();

	for (const space of await root.listSpaces(SPACE_TYPE.epk)) {
		// A legacy press kit's space, which the root writes in too, moves with the press kit on its maker's visit.
		if (parseSpaceUri(space)?.authority !== root.did) continue;

		try {
			const policy = (await root.getSpace(space))?.policy;

			if (!policy || !('publicWrite' in policy) || !policy.publicWrite) continue;
			await moveSpace(core, epks, space);
		} catch (error) {
			core.logger.warn({ err: error, space }, "could not move a press kit's requests; the next start tries again");
		}
	}
}

async function moveSpace(core: Core, epks: EpkService, space: string): Promise<void> {
	const root = await core.root();
	const kit = await epks.locate(root.did, parseSpaceUri(space)!.skey);

	if (!kit || kit.epk.space !== space) {
		core.logger.warn({ space }, 'leaving the requests of a press kit space with no press kit');

		return;
	}

	const rows = await core.db.selectFrom('epkRequest').selectAll().where('spaceUri', '=', space).execute();
	let left = 0;

	for (const row of rows) {
		const old = parseSpaceRecordUri(row.uri)!;
		const writer = old.author === root.did ? root : await core.userClient(old.author);

		if (!writer) {
			core.logger.info({ uri: row.uri }, 'a request stays in its press kit space until its asker signs in again');
			left++;
			continue;
		}

		const copy = await epks.copyRequest(row, kit.project, kit.epk.uri);

		try {
			await writer.deleteSpaceRecord({ space, collection: COLLECTION.epkRequest, rkey: old.rkey });
		} catch (error) {
			// A request moves whole or not at all, so the managers never see it twice.
			const made = parseSpaceRecordUri(copy)!;
			await root.deleteSpaceRecord({ space: spaceOfRecord(copy)!, collection: made.collection, rkey: made.rkey });
			await core.db.deleteFrom('epkRequest').where('uri', '=', copy).execute();
			core.logger.warn({ err: error, uri: row.uri }, 'could not delete a request from its press kit space');
			left++;
			continue;
		}

		const grants = await core.db.selectFrom('epkGrant').selectAll().where('requestUri', '=', row.uri).execute();

		for (const grant of grants) {
			const via = grant.via === 'auto' || grant.via === 'invited' ? grant.via : 'approved';
			await epks.writeGrant(space, { subject: grant.subjectDid, via, request: copy, createdAt: grant.createdAt });
		}

		await core.db.deleteFrom('epkRequest').where('uri', '=', row.uri).execute();
	}

	if (left > 0) return;
	await root.updateSpace({ space, policy: memberSpacePolicy() });
	core.logger.info({ space, moved: rows.length }, "moved a press kit's requests to its project's request space");
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
