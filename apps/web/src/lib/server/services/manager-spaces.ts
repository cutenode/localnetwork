import { COLLECTION } from '#lib/nsid.ts';
import type { RootRepoClient } from '../atproto/repo-client';
import { backstageSpace, epkSpace, parsePublicRecordUri } from '../atproto/spaces';
import type { Core } from '../core';
import type { IndexedRepo } from '../happyview/indexed';
import { auditedProjects, openedProjects } from './bookings';
import { type MemberSync, reconcileMembers } from './space-members';

/**
 * Whether a manager who joins the project now has a part in a booking: it's still being worked out, or it's confirmed
 * for a show that hasn't happened yet (or has no date), which the team still announces and plays. Past shows,
 * declines, and cancellations are settled, and their threads stay with the managers who had them.
 */
function isLiveBooking(status: string, showDate: string | null): boolean {
	if (status === 'open' || status === 'negotiating') return true;

	return status === 'confirmed' && (showDate === null || Date.parse(showDate) > Date.now());
}

/** A space whose member list follows a project's managers. A manager who leaves the project leaves every space. */
type ManagerSpace = Omit<MemberSync, 'wanted'> & {
	/** Whether managers write to it, or only read it. */
	write: boolean;
};

/**
 * Keeps the member lists of a project's spaces in step with its managers. Each sync compares the managers with a
 * space's live member list rather than with the last change, so running it again does nothing, and a sync that
 * failed part way is finished by the next one.
 */
export class ManagerSpaces {
	constructor(
		private readonly core: Core,
		/** Where the root's press kits are read; a HappyView that's down fails the sync, which runs again later. */
		private readonly indexed: IndexedRepo
	) {}

	async sync(projectUri: string, managers: string[]): Promise<void> {
		for (const space of await this.spaces(projectUri)) await this.syncSpace(space, managers);
	}

	private async spaces(projectUri: string): Promise<ManagerSpace[]> {
		return [...(await this.bookingSpaces(projectUri)), ...(await this.pressKitSpaces(projectUri))];
	}

	/**
	 * The project's backstage, once a manager has made one, where managers read and write, and the spaces of the press
	 * kits the root holds for it, which managers read beside the people granted access, who stay.
	 */
	private async pressKitSpaces(projectUri: string): Promise<ManagerSpace[]> {
		const project = parsePublicRecordUri(projectUri);

		if (!project) return [];
		const root = await this.core.root();
		const spaces: ManagerSpace[] = [];
		const backstage = backstageSpace(root.did, project.rkey);

		if (await root.getSpace(backstage)) spaces.push({ uri: backstage, joinable: true, write: true, others: [] });

		for (const { uri, value } of await this.indexed.listAll(root.did, COLLECTION.epk, { strict: true })) {
			if (value.project !== projectUri) continue;
			const space = epkSpace(root.did, uri.split('/').at(-1)!);

			try {
				spaces.push({ uri: space, joinable: true, write: false, others: await this.grantees(root, space) });
			} catch (error) {
				// Without the grants, the sync can't tell a grantee from a manager who left, so it leaves the space for next time.
				this.core.logger.warn({ err: error, space }, "could not read a press kit's grants");
			}
		}

		return spaces;
	}

	/** Who the root's grant records in a press kit's space let in. */
	private async grantees(root: RootRepoClient, space: string): Promise<string[]> {
		const subjects: string[] = [];
		let cursor: string | undefined;

		do {
			const page = await root.listSpaceRecords({ space, repo: root.did, collection: COLLECTION.epkGrant, cursor });
			subjects.push(...page.records.map((record) => record.rkey));
			cursor = page.cursor;
		} while (cursor);

		return subjects;
	}

	/**
	 * The project's booking threads: the promoter stays in each, and only live ones take new managers. The promoter can
	 * rewrite their inquiry to point anywhere, so a thread counts only when a record the root wrote ties it to the
	 * project: its opening record, or for a thread opened before that named one, its audit entry.
	 */
	private async bookingSpaces(projectUri: string): Promise<ManagerSpace[]> {
		const { db } = this.core;

		const named = await Promise.all([
			db.selectFrom('bookingThread').select('spaceUri').where('projectUri', '=', projectUri).execute(),
			openedProjects(this.core, projectUri).execute(),
			auditedProjects(this.core, projectUri).execute()
		]);

		const spaces = [...new Set(named.flat().map((row) => row.spaceUri))];

		if (spaces.length === 0) return [];

		const [threads, opened, audited] = await Promise.all([
			db
				.selectFrom('bookingThread')
				.select(['spaceUri', 'promoterDid', 'status', 'showDate'])
				.where('spaceUri', 'in', spaces)
				.execute(),
			openedProjects(this.core).where('spaceUri', 'in', spaces).execute(),
			auditedProjects(this.core).where('subject', 'in', spaces).execute()
		]);

		const openedBy = new Map(opened.map((row) => [row.spaceUri, row.project]));
		const auditedBy = new Map(audited.map((row) => [row.spaceUri, row.project]));

		return threads.flatMap((thread) => {
			const space = thread.spaceUri;
			const [fromOpening, fromAudit] = [openedBy.get(space), auditedBy.get(space)];

			if (fromOpening && fromAudit && fromOpening !== fromAudit) {
				this.core.logger.warn(
					{ space, opening: fromOpening, audit: fromAudit },
					'skipping a booking thread whose opening record and audit entry name different projects'
				);

				return [];
			}

			if ((fromOpening ?? fromAudit) !== projectUri) {
				this.core.logger.info(
					{ space, project: projectUri },
					'skipping a booking thread no root record ties to the project'
				);

				return [];
			}

			return [
				{
					uri: space,
					joinable: isLiveBooking(thread.status, thread.showDate),
					write: true,
					others: [thread.promoterDid],
					ledger: {
						recorded: () => this.recordedBookingManagers(space),
						record: (managers: string[]) => this.recordBookingManagers(space, managers)
					}
				}
			];
		});
	}

	private async syncSpace(space: ManagerSpace, managers: string[]): Promise<void> {
		const access = { read: true, write: space.write };
		await reconcileMembers(this.core, { ...space, wanted: new Map(managers.map((did) => [did, access])) });
	}

	private async recordedBookingManagers(space: string): Promise<string[]> {
		const rows = await this.core.db
			.selectFrom('bookingParticipant')
			.select('did')
			.where('spaceUri', '=', space)
			.where('role', '=', 'manager')
			.execute();

		return rows.map((row) => row.did);
	}

	private async recordBookingManagers(space: string, managers: string[]): Promise<void> {
		await this.core.db.transaction().execute(async (trx) => {
			let stale = trx.deleteFrom('bookingParticipant').where('spaceUri', '=', space).where('role', '=', 'manager');

			if (managers.length > 0) stale = stale.where('did', 'not in', managers);
			await stale.execute();

			if (managers.length === 0) return;
			await trx
				.insertInto('bookingParticipant')
				.values(managers.map((did) => ({ spaceUri: space, did, role: 'manager' as const })))
				.onConflict((oc) => oc.doNothing())
				.execute();
		});
	}
}
