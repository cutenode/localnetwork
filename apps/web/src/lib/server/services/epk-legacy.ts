import { epkGrantSchema } from '#lib/epk-schema.ts';
import { type EpkView, grantVia } from '#lib/epk.ts';
import { COLLECTION, EPK_CONTENT_KEY } from '#lib/nsid.ts';
import type { ProjectView, Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { listAllSpaceRecords, type RepoClient } from '../atproto/repo-client';
import type { Core } from '../core';
import { nowIso } from '../db';
import type { EpkService } from './epk';
import { toJson } from './epk-fields';
import type { ProjectService } from './projects';

/** A legacy press kit as the root holds it once moved. */
type MovedKit = Pick<EpkView, 'uri' | 'rkey' | 'space'>;

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
		const kits = await this.epks.publishedBy(viewer.did, true);

		if (kits.length === 0) return result;
		const owner = await this.core.userClient(viewer.did);

		if (!owner) {
			this.core.logger.info('legacy press kits wait for their maker to sign in again', {
				did: viewer.did,
				kits: kits.length
			});

			return result;
		}

		for (const epk of kits) {
			try {
				if (await this.move(owner, viewer, epk)) result.migrated.push(epk.rkey);
			} catch (error) {
				this.core.logger.warn('could not move a legacy press kit; the next visit tries again', {
					err: error,
					epk: epk.uri
				});
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
			this.core.logger.warn('leaving a legacy press kit whose key the root holds for another project', {
				...log,
				held: held.uri
			});

			return false;
		}

		if (!held) {
			const project = await this.projects.current(epk.record.project);

			if (!project?.managers.includes(viewer.did)) {
				this.core.logger.info("leaving a legacy press kit of a project its maker doesn't manage", log);

				return false;
			}

			// Another manager's legacy press kit under the same key means one copied the other's key. Moving either would make
			// the other look moved, and its maker's next visit would delete it, so both stay where they are.
			const others = project.managers.filter((did) => did !== viewer.did);
			const lists = await Promise.all(others.map((did) => this.epks.publishedBy(did, true)));

			if (lists.flat().some((other) => other.rkey === epk.rkey)) {
				this.core.logger.warn("leaving a legacy press kit another manager's press kit has the key of", log);

				return false;
			}

			const root = await this.core.root();
			const content = await this.epks.content(epk);

			for (const photo of epk.record.photos ?? []) await this.epks.copyPhoto(root, photo, { did: epk.did });

			for (const photo of content?.photos ?? [])
				await this.epks.copyPhoto(root, photo, { did: epk.did, space: epk.space });
			this.core.logger.info("copied a legacy press kit's photos to the root", log);

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

			const moved = { uri, rkey: epk.rkey, space };
			const requests = await this.carryRequests(epk, project, moved);
			await this.carryGrants(owner, epk, project, moved, requests);
			this.core.logger.info("copied a legacy press kit's private part, requests and grants", {
				...log,
				space,
				requests: requests.size
			});

			const ref = await root.putRecord({
				collection: COLLECTION.epk,
				rkey: epk.rkey,
				record: validateRecord(COLLECTION.epk, toJson(epk.record)),
				swapRecord: null
			});

			await this.epks.indexed.awaitWrite(root.did, COLLECTION.epk, epk.rkey, ref.cid);
			this.core.logger.info('moved a legacy press kit to the root', log);
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
		await this.epks.indexed.awaitWrite(owner.did, COLLECTION.epk, epk.rkey, null);
		await this.epks.forgetIndex(epk.space);
		this.core.logger.info("deleted a moved press kit's old record and space", log);

		return true;
	}

	/**
	 * Moves the press kit's requests over to the root's copy: the open ones still in the old space are rewritten into the
	 * project's request space as the root, on the askers' behalf, and the ones made there since only change press kit.
	 * Grants the open ones the rules approved, which waited for the move. Returns each old request's URI now.
	 */
	private async carryRequests(epk: EpkView, project: ProjectView, moved: MovedKit): Promise<Map<string, string>> {
		const carried = new Map<string, string>();
		const rows = await this.core.db.selectFrom('epkRequest').selectAll().where('epkUri', '=', epk.uri).execute();

		for (const row of rows) {
			const inOldSpace = row.spaceUri === epk.space;

			if (inOldSpace && row.status !== 'pending') continue;

			if (inOldSpace) carried.set(row.uri, await this.epks.copyRequest(row, project, moved.uri));
			else {
				await this.core.db.updateTable('epkRequest').set({ epkUri: moved.uri }).where('uri', '=', row.uri).execute();
				carried.set(row.uri, row.uri);
			}

			if (row.status === 'pending' && row.rule === 'auto')
				await this.epks.writeGrant(project, moved, {
					subject: row.requesterDid,
					via: 'auto',
					request: carried.get(row.uri),
					createdAt: nowIso()
				});
		}

		return carried;
	}

	/**
	 * Rewrites the maker's grants as the root's, read from their repo in the old space, into the project's request space.
	 */
	private async carryGrants(
		owner: RepoClient,
		epk: EpkView,
		project: ProjectView,
		moved: MovedKit,
		requests: Map<string, string>
	): Promise<void> {
		const records = await listAllSpaceRecords(owner, {
			space: epk.space,
			repo: owner.did,
			collection: COLLECTION.epkGrant
		});

		for (const record of records) {
			const grant = epkGrantSchema.safeParse(record.value);

			if (!grant.success) continue;
			const { subject, via, request, createdAt } = grant.data;

			await this.epks.writeGrant(project, moved, {
				subject,
				via: grantVia(via),
				// The old request goes with the old space, unless it was still open and moved too.
				request: request && requests.get(request),
				createdAt
			});
		}
	}
}
