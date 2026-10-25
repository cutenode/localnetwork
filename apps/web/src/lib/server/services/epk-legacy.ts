import { epkGrantSchema } from '#lib/epk-schema.ts';
import type { EpkView } from '#lib/epk.ts';
import { COLLECTION, EPK_CONTENT_KEY } from '#lib/nsid.ts';
import type { Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import type { RepoClient } from '../atproto/repo-client';
import type { Core } from '../core';
import { nowIso } from '../db';
import type { EpkService } from './epk';
import { toJson } from './epk-fields';
import type { ProjectService } from './projects';

/** The keys of the press kits a visit moved, and of those it couldn't. */
export type LegacyMigration = { migrated: string[]; failed: string[] };

/**
 * Moves press kits made before the root held them, which sit in their maker's repo and a space under their DID, to the
 * root with the same key. Only the maker's session can delete the old space, so each moves on its maker's next visit
 * to the dashboard.
 */
export class LegacyEpks {
	/** Each account's move in progress, which a second visit joins instead of racing it. */
	private readonly running = new Map<string, Promise<LegacyMigration>>();

	constructor(
		private readonly core: Core,
		private readonly epks: EpkService,
		private readonly projects: ProjectService
	) {}

	/**
	 * Moves `viewer`'s legacy press kits, one at a time, and says which moved and which failed. Every step writes the
	 * same thing when it runs again, and the public record goes into the root's repo only after everything else, so a
	 * press kit whose key the root holds has moved, and only its old copy is left to delete. A failure is logged, and the
	 * next visit picks up where it stopped. Press kits of projects `viewer` no longer manages stay where they are.
	 */
	migrate(viewer: Viewer): Promise<LegacyMigration> {
		const running = this.running.get(viewer.did);

		if (running) return running;
		const migration = this.moveAll(viewer).finally(() => this.running.delete(viewer.did));
		this.running.set(viewer.did, migration);

		return migration;
	}

	private async moveAll(viewer: Viewer): Promise<LegacyMigration> {
		const result: LegacyMigration = { migrated: [], failed: [] };

		// Making a press kit needed spaces, so an account without them has none to move.
		if (!viewer.spacesCapable) return result;
		this.epks.forget(viewer.did);
		const kits = await this.epks.publishedBy(viewer.did);

		if (kits.length === 0) return result;
		const owner = await this.core.userClient(viewer.did);

		if (!owner) {
			this.core.logger.info(
				{ did: viewer.did, kits: kits.length },
				'legacy press kits wait for their maker to sign in again'
			);

			return result;
		}

		for (const epk of kits) {
			try {
				if (await this.move(owner, viewer, epk)) result.migrated.push(epk.rkey);
			} catch (error) {
				this.core.logger.warn(
					{ err: error, epk: epk.uri },
					'could not move a legacy press kit; the next visit tries again'
				);
				result.failed.push(epk.rkey);
			}
		}

		this.epks.forget();
		this.epks.forget(viewer.did);

		return result;
	}

	/** Moves one press kit, or finishes moving it, and says whether it did. */
	private async move(owner: RepoClient, viewer: Viewer, epk: EpkView): Promise<boolean> {
		const log = { epk: epk.uri, did: viewer.did };
		const held = await this.epks.rootKit(epk.rkey);

		// The root's press kit under this key is this one moved only if it's of the same project. Anyone can pick a key in
		// their own repo, so one of another project means a clash, and deleting the maker's copy would lose it.
		if (held && held.record.project !== epk.record.project) {
			this.core.logger.warn(
				{ ...log, held: held.uri },
				'leaving a legacy press kit whose key the root holds for another project'
			);

			return false;
		}

		if (!held) {
			const project = await this.projects.current(epk.record.project);

			if (!project?.managers.includes(viewer.did)) {
				this.core.logger.info(log, "leaving a legacy press kit of a project its maker doesn't manage");

				return false;
			}

			// Another manager's legacy press kit under the same key means one copied the other's key. Moving either would make
			// the other look moved, and its maker's next visit would delete it, so both stay where they are.
			const others = project.managers.filter((did) => did !== viewer.did);
			const lists = await Promise.all(others.map((did) => this.epks.publishedBy(did)));

			if (lists.flat().some((other) => other.rkey === epk.rkey)) {
				this.core.logger.warn(log, "leaving a legacy press kit another manager's press kit has the key of");

				return false;
			}

			const root = await this.core.root();
			const content = await this.epks.content(epk);

			for (const photo of epk.record.photos ?? []) await this.epks.copyPhoto(root, photo, { did: epk.did });

			for (const photo of content?.photos ?? [])
				await this.epks.copyPhoto(root, photo, { did: epk.did, space: epk.space });
			this.core.logger.info(log, "copied a legacy press kit's photos to the root");

			const space = await this.epks.ensureKitSpace(epk.rkey, project.managers);
			const uri = `at://${root.did}/${COLLECTION.epk}/${epk.rkey}`;

			if (content) {
				await root.putSpaceRecord({
					space,
					collection: COLLECTION.epkContent,
					rkey: EPK_CONTENT_KEY,
					record: validateRecord(COLLECTION.epkContent, toJson(content))
				});
			}

			const requests = await this.carryRequests(epk, space, uri);
			await this.carryGrants(owner, epk, space, requests);
			this.core.logger.info(
				{ ...log, space, requests: requests.size },
				"copied a legacy press kit's private part, requests and grants"
			);

			await root.putRecord({
				collection: COLLECTION.epk,
				rkey: epk.rkey,
				record: validateRecord(COLLECTION.epk, toJson(epk.record)),
				swapRecord: null
			});
			this.core.logger.info(log, 'moved a legacy press kit to the root');
			await this.core.audit.record({
				actor: viewer.did,
				actorRole: 'member',
				action: 'epk.save',
				subject: uri,
				subjectKind: 'epk',
				summary: `Moved the press kit “${epk.record.title}” to ${project.profile.name}, for all its managers`,
				details: { project: project.uri, from: epk.uri }
			});
		}

		// The old copy goes last. Only the space's authority, its maker, can delete it.
		if (await owner.getSpace(epk.space)) await owner.deleteSpace(epk.space);
		await owner.deleteRecord({ collection: COLLECTION.epk, rkey: epk.rkey });
		await this.epks.forgetIndex(epk.space);
		this.core.logger.info(log, "deleted a moved press kit's old record and space");

		return true;
	}

	/**
	 * Rewrites the old space's open requests into the new one as the root, on the askers' behalf,
	 * and grants the ones the rules approved, which waited for the move. Returns each old request's new URI.
	 */
	private async carryRequests(epk: EpkView, space: string, uri: string): Promise<Map<string, string>> {
		const root = await this.core.root();
		const carried = new Map<string, string>();

		const rows = await this.core.db
			.selectFrom('epkRequest')
			.selectAll()
			.where('spaceUri', '=', epk.space)
			.where('status', '=', 'pending')
			.execute();

		for (const row of rows) {
			const record = toJson({
				epk: uri,
				role: row.role ?? undefined,
				message: row.message ?? undefined,
				onBehalfOf: row.requesterDid,
				createdAt: row.createdAt
			});

			// Each asker picked their own request's key, so two can share one; the root writes each under a key of its own,
			// once per asker, which an earlier, unfinished move may have done already.
			const moved = await this.core.db
				.selectFrom('epkRequest')
				.select('uri')
				.where('spaceUri', '=', space)
				.where('requesterDid', '=', row.requesterDid)
				.executeTakeFirst();

			const ref =
				moved ??
				(await root.createSpaceRecord({
					space,
					collection: COLLECTION.epkRequest,
					record: validateRecord(COLLECTION.epkRequest, record)
				}));

			await this.epks.saveRequest({
				uri: ref.uri,
				space,
				epk: uri,
				requester: row.requesterDid,
				role: row.role,
				message: row.message,
				rule: row.rule,
				createdAt: row.createdAt
			});
			carried.set(row.uri, ref.uri);

			if (row.rule === 'auto')
				await this.epks.writeGrant(space, {
					subject: row.requesterDid,
					via: 'auto',
					request: ref.uri,
					createdAt: nowIso()
				});
		}

		return carried;
	}

	/** Rewrites the maker's grants as the root's, read from their repo in the old space. */
	private async carryGrants(
		owner: RepoClient,
		epk: EpkView,
		space: string,
		requests: Map<string, string>
	): Promise<void> {
		let cursor: string | undefined;

		do {
			const page = await owner.listSpaceRecords({
				space: epk.space,
				repo: owner.did,
				collection: COLLECTION.epkGrant,
				cursor
			});

			for (const record of page.records) {
				const grant = epkGrantSchema.safeParse(record.value);

				if (!grant.success) continue;
				const { subject, via, request, createdAt } = grant.data;

				await this.epks.writeGrant(space, {
					subject,
					via: via === 'auto' || via === 'invited' ? via : 'approved',
					// The old request goes with the old space, unless it was still open and moved too.
					request: request && requests.get(request),
					createdAt
				});
			}

			cursor = page.cursor;
		} while (cursor);
	}
}
