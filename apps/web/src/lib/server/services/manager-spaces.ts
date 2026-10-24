import type { Core } from '../core';

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
	/** Keeps the app's own record of which managers are members, which its access checks read. */
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

	/** The project's booking threads: the promoter stays in each, and only live ones take new managers. */
	private async spaces(projectUri: string): Promise<ManagerSpace[]> {
		const threads = await this.core.db
			.selectFrom('bookingThread')
			.select(['spaceUri', 'promoterDid', 'status', 'showDate'])
			.where('projectUri', '=', projectUri)
			.execute();

		return threads.map((thread) => ({
			uri: thread.spaceUri,
			joinable: isLiveBooking(thread.status, thread.showDate),
			others: [thread.promoterDid],
			record: (managers) => this.recordBookingManagers(thread.spaceUri, managers)
		}));
	}

	private async syncSpace(space: ManagerSpace, managers: string[]): Promise<void> {
		const root = await this.core.root();
		let members: Set<string>;

		try {
			members = new Set((await root.listMembers(space.uri)).map((member) => member.did));
		} catch (error) {
			this.core.logger.warn({ err: error, space: space.uri }, "could not read a space's members");

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
