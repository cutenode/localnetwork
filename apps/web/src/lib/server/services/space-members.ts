import type { Core } from '../core';

export type MemberAccess = { read: boolean; write: boolean };

/** A space whose member list the app keeps in step with a list of the people who should be in it. */
export type MemberSync = {
	uri: string;
	/** Who should be a member, and with what access. */
	wanted: Map<string, MemberAccess>;
	/** Members who aren't there through `wanted`, like a booking's promoter, so the sync never adds or removes them. */
	others: string[];
	/** Whether someone wanted who isn't a member yet joins. Someone who's no longer wanted leaves either way. */
	joinable: boolean;
	/** The app's own record of the wanted members it has, for a space whose access checks read that, not the space. */
	ledger?: {
		recorded(): Promise<string[]>;
		record(members: string[]): Promise<void>;
	};
};

const sameAccess = (a: MemberAccess, b: MemberAccess) => a.read === b.read && a.write === b.write;

/**
 * Brings a space's member list in line with `space.wanted`: adds who's missing, removes who's no longer wanted, and
 * corrects anyone whose read or write access drifted. It compares with the live member list rather than the last
 * change, so running it again does nothing, and a run that failed part way is finished by the next one.
 */
export async function reconcileMembers(core: Core, space: MemberSync): Promise<void> {
	const root = await core.root();
	let members: Map<string, MemberAccess>;

	try {
		members = new Map((await root.listMembers(space.uri)).map(({ did, read, write }) => [did, { read, write }]));
	} catch (error) {
		// Fail closed: someone no longer wanted still loses access in the app, and nobody joins until the space can be read.
		core.logger.warn({ err: error, space: space.uri }, "could not read a space's members");

		if (!space.ledger) return;
		const recorded = new Set(await space.ledger.recorded());
		await space.ledger.record([...space.wanted.keys()].filter((did) => recorded.has(did)));

		return;
	}

	const others = new Set([core.rootDid, ...space.others]);

	const wanted = new Map([...space.wanted].filter(([did]) => !others.has(did) && (space.joinable || members.has(did))));

	const changing = [...wanted].filter(([did, access]) => {
		const current = members.get(did);

		return !current || !sameAccess(current, access);
	});

	const leaving = [...members.keys()].filter((did) => !others.has(did) && !wanted.has(did));
	const joined: string[] = [];
	const changed: string[] = [];
	const left: string[] = [];

	for (const [did, access] of changing) {
		try {
			await root.putMember({ space: space.uri, did, ...access });
			(members.has(did) ? changed : joined).push(did);
		} catch (error) {
			core.logger.warn({ err: error, space: space.uri, did }, "could not update someone's access to a space");
		}
	}

	for (const did of leaving) {
		try {
			await root.removeMember({ space: space.uri, did });
			left.push(did);
		} catch (error) {
			core.logger.warn({ err: error, space: space.uri, did }, 'could not remove someone from a space');
		}
	}

	// Someone who couldn't be removed still loses access in the app now; the next sync removes them from the space.
	await space.ledger?.record([...wanted.keys()].filter((did) => members.has(did) || joined.includes(did)));

	if (joined.length > 0 || changed.length > 0 || left.length > 0)
		core.logger.info({ space: space.uri, joined, changed, left }, "updated a space's members");
}
