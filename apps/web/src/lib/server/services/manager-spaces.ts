import type { Core } from '../core';
import { auditedProjects, openedProjects } from './bookings';

/**
 * Whether a manager who joins the project now has a part in a booking: it's still being worked out, or it's confirmed
 * for a show that hasn't happened yet (or has no date), which the team still announces and plays. Past shows,
 * declines, and cancellations are settled, and their threads stay with the managers who had them.
 */
function isLiveBooking(status: string, showDate: string | null): boolean {
	if (status === 'open' || status === 'negotiating') return true;

	return status === 'confirmed' && (showDate === null || Date.parse(showDate) > Date.now());
}

/** A space whose member list follows a project's managers. */
type ManagerSpace = {
	uri: string;
	/** Whether a manager who isn't a member yet joins it. A manager who leaves the project leaves every space. */
	joinable: boolean;
	/** Members who aren't there as managers, like a booking's promoter, so the sync never adds or removes them. */
	others: string[];
	/** The managers the app's own record has as members, which its access checks read. */
	recorded(): Promise<string[]>;
	/** Keeps that record. */
	record(managers: string[]): Promise<void>;
};

/**
 * Keeps the member lists of a project's spaces in step with its managers. Each sync compares the managers with a
 * space's live member list rather than with the last change, so running it again does nothing, and a sync that
 * failed part way is finished by the next one.
 */
export class ManagerSpaces {
	constructor(private readonly core: Core) {}

	async sync(projectUri: string, managers: string[]): Promise<void> {
		for (const space of await this.spaces(projectUri)) await this.syncSpace(space, managers);
	}

	/**
	 * The project's booking threads: the promoter stays in each, and only live ones take new managers. The promoter can
	 * rewrite their inquiry to point anywhere, so a thread counts only when a record the root wrote ties it to the
	 * project: its opening record, or for a thread opened before that named one, its audit entry.
	 */
	private async spaces(projectUri: string): Promise<ManagerSpace[]> {
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
					others: [thread.promoterDid],
					recorded: () => this.recordedBookingManagers(space),
					record: (managers) => this.recordBookingManagers(space, managers)
				}
			];
		});
	}

	private async syncSpace(space: ManagerSpace, managers: string[]): Promise<void> {
		const root = await this.core.root();
		let members: Set<string>;

		try {
			members = new Set((await root.listMembers(space.uri)).map((member) => member.did));
		} catch (error) {
			// Fail closed: a manager who left still loses access in the app, and nobody joins until the space can be read.
			this.core.logger.warn({ err: error, space: space.uri }, "could not read a space's members");
			const recorded = new Set(await space.recorded());
			await space.record(managers.filter((did) => recorded.has(did)));

			return;
		}

		const others = new Set([this.core.rootDid, ...space.others]);
		const wanted = managers.filter((did) => !others.has(did) && (space.joinable || members.has(did)));
		const joining = wanted.filter((did) => !members.has(did));
		const leaving = [...members].filter((did) => !others.has(did) && !wanted.includes(did));
		const joined: string[] = [];
		const left: string[] = [];

		for (const did of joining) {
			try {
				await root.putMember({ space: space.uri, did, read: true, write: true });
				joined.push(did);
			} catch (error) {
				this.core.logger.warn({ err: error, space: space.uri, did }, 'could not add a manager to a space');
			}
		}

		for (const did of leaving) {
			try {
				await root.removeMember({ space: space.uri, did });
				left.push(did);
			} catch (error) {
				this.core.logger.warn({ err: error, space: space.uri, did }, 'could not remove a manager from a space');
			}
		}

		// A manager who couldn't be removed still loses access in the app now; the next sync removes them from the space.
		await space.record(wanted.filter((did) => members.has(did) || joined.includes(did)));

		if (joined.length > 0 || left.length > 0)
			this.core.logger.info({ space: space.uri, joined, left }, "updated a space's managers");
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
