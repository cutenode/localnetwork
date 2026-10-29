import { TID } from '@atproto/common-web';
import type { JsonObject } from '@atproto/lex-json';
import { z } from 'zod';
import {
	type Capability,
	capabilitiesFor,
	capabilitiesIn,
	CREATOR_ROLE,
	isRole,
	isWriter,
	type SpaceKind
} from '#lib/capabilities.ts';
import { COLLECTION } from '#lib/nsid.ts';
import type { AccountView, Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import type { RootRepoClient } from '../atproto/repo-client';
import { parseSpaceUri, SHARED_SPACE_TYPES, sharedSpace, sharedSpaceKind } from '../atproto/spaces';
import type { ActorRole, AuditAction, AuditChange } from '../audit/log';
import { type Core, ForbiddenError, NotFoundError, UserError } from '../core';
import { nowIso, parseJson } from '../db';
import { actorRole } from './requests';
import { reconcileMembers } from './space-members';

/** The one listing status record's key in each space. */
const LISTING_STATUS_KEY = 'self';

const memberList = { $type: 'com.atproto.simplespace.defs#memberListPolicy' } as const;

const personRecord = z.object({
	subject: z.string(),
	role: z.string(),
	capabilities: z.array(z.string()),
	hidden: z.boolean(),
	via: z.string(),
	addedBy: z.string().optional(),
	source: z.string().optional(),
	createdAt: z.string(),
	updatedAt: z.string()
});

type PersonRecord = z.infer<typeof personRecord>;

const inviteRecord = z.object({
	subject: z.string(),
	role: z.string(),
	capabilities: z.array(z.string()),
	invitedBy: z.string(),
	message: z.string().optional(),
	createdAt: z.string(),
	expiresAt: z.string().optional()
});

const inviteLinkRecord = z.object({
	tokenHash: z.string(),
	role: z.string(),
	capabilities: z.array(z.string()),
	createdBy: z.string(),
	createdAt: z.string(),
	expiresAt: z.string().optional()
});

const joinRequestRecord = z.object({ subject: z.string(), message: z.string().optional(), createdAt: z.string() });

const listingStatusRecord = z.object({
	listed: z.boolean(),
	createdBy: z.string(),
	createdAt: z.string(),
	decidedBy: z.string().optional(),
	decidedAt: z.string().optional(),
	request: z.string().optional()
});

type ListingStatusRecord = z.infer<typeof listingStatusRecord>;

export type SharedSpaceView = { uri: string; kind: SpaceKind; listed: boolean; createdBy: string; createdAt: string };

export type PersonView = AccountView & {
	role: string;
	capabilities: Capability[];
	hidden: boolean;
	via: string;
	addedBy: string | null;
	createdAt: string;
	updatedAt: string;
};

/** A record in a shared space. */
type SpaceRecordChange = { space: string; collection: string; rkey: string; value: JsonObject };

/** What a change sees and does: the space's kind, and its people as the root's repo has them now, by DID. */
type Live = { space: string; kind: SpaceKind; root: RootRepoClient; people: Map<string, PersonRecord> };

const hasCapability = (kind: SpaceKind, person: PersonRecord | undefined, capability: Capability) =>
	!!person && capabilitiesIn(kind, person.capabilities).includes(capability);

/**
 * Performers', parties' and venues' spaces under the root, and the people in them. The root writes every record in
 * them, after checking the person it writes for holds the capability the change needs; staff can make any change,
 * and the audit log notes their role. Each space's members follow its people: everyone reads, and those with a
 * capability that writes also write.
 */
export class SharedSpaces {
	/**
	 * One change at a time in each space, so a check like "someone can still manage its people" sees the change before
	 * it. The lock is in this process, which assumes one app instance, as the sync engine's space locks do.
	 */
	private readonly locks = new Map<string, Promise<void>>();

	constructor(private readonly core: Core) {}

	/** Starts a space of `kind` with `actor` in it, in the role that gets every capability of its kind. */
	async create(actor: Viewer, kind: SpaceKind): Promise<string> {
		const root = await this.core.root();
		const skey = TID.nextStr();
		const space = sharedSpace(root.did, kind, skey);
		const now = nowIso();
		const role = CREATOR_ROLE[kind];

		const person: PersonRecord = {
			subject: actor.did,
			role,
			capabilities: capabilitiesFor(kind, role),
			hidden: false,
			via: 'created',
			createdAt: now,
			updatedAt: now
		};

		const status: ListingStatusRecord = { listed: false, createdBy: actor.did, createdAt: now };

		await root.createSpace({
			spaceType: SHARED_SPACE_TYPES[kind],
			skey,
			readPolicy: memberList,
			writePolicy: memberList,
			appAccess: { $type: 'com.atproto.simplespace.defs#open' }
		});
		await root.applySpaceWrites(space, [
			{
				action: 'create',
				collection: COLLECTION.listingStatus,
				rkey: LISTING_STATUS_KEY,
				record: validateRecord(COLLECTION.listingStatus, status)
			},
			{
				action: 'create',
				collection: COLLECTION.person,
				rkey: actor.did,
				record: validateRecord(COLLECTION.person, person)
			}
		]);
		await this.saveStatus(space, kind, status);
		await this.savePerson(space, kind, person);
		await this.syncPeople({ space, kind, root, people: new Map([[actor.did, person]]) });
		await this.core.watchSpace?.(space);
		this.core.logger.info({ space, kind }, 'created a shared space');
		await this.record(actor, 'member', 'space.create', space, `Started a ${kind}`, { person: actor.did, role });

		return space;
	}

	async get(space: string): Promise<SharedSpaceView | null> {
		const row = await this.core.db
			.selectFrom('sharedSpace')
			.selectAll()
			.where('spaceUri', '=', space)
			.executeTakeFirst();

		return row
			? {
					uri: row.spaceUri,
					kind: row.kind,
					listed: row.listed === 1,
					createdBy: row.createdBy,
					createdAt: row.createdAt
				}
			: null;
	}

	/** The space's people, oldest first. Someone hidden is left out unless `viewer` is in the space or staff. */
	async people(space: string, viewer: Viewer | null): Promise<PersonView[]> {
		const rows = await this.core.db
			.selectFrom('spacePerson')
			.selectAll()
			.where('spaceUri', '=', space)
			.orderBy('createdAt')
			.execute();

		const insider = !!viewer && (viewer.isStaff || rows.some((row) => row.did === viewer.did));
		const shown = rows.filter((row) => insider || row.hidden === 0);
		const accounts = await this.core.accounts.views(shown.map((row) => row.did));

		return shown.map((row) => ({
			did: row.did,
			handle: accounts.get(row.did)?.handle ?? null,
			role: row.role,
			capabilities: parseJson<Capability[]>(row.capabilities, []),
			hidden: row.hidden === 1,
			via: row.via,
			addedBy: row.addedBy,
			createdAt: row.createdAt,
			updatedAt: row.updatedAt
		}));
	}

	/** Staff add someone straight to the space, in one of its kind's roles. */
	async addPerson(actor: Viewer, space: string, input: { did: string; role: string; hidden?: boolean }): Promise<void> {
		if (!actor.isStaff) throw new ForbiddenError('Only staff can add people directly');

		await this.change(space, async (live) => {
			if (!isRole(live.kind, input.role)) throw new UserError(`A ${live.kind} has no ${input.role} role`);

			if (live.people.has(input.did)) throw new UserError("They're already in it");
			const now = nowIso();

			const person: PersonRecord = {
				subject: input.did,
				role: input.role,
				capabilities: capabilitiesFor(live.kind, input.role),
				hidden: input.hidden ?? false,
				via: 'staff',
				addedBy: actor.did,
				createdAt: now,
				updatedAt: now
			};

			await this.writePerson(live, person);
			await this.record(actor, actorRole(actor), 'people.add', space, `Added ${await this.name(input.did)}`, {
				person: input.did,
				role: person.role,
				capabilities: person.capabilities,
				hidden: person.hidden
			});
		});
	}

	/**
	 * Changes someone's role, which gives them its capabilities, or their capabilities. Needs people.manage, and
	 * someone has to be left with it.
	 */
	async updatePerson(
		actor: Viewer,
		space: string,
		did: string,
		update: { role?: string; capabilities?: string[] }
	): Promise<void> {
		await this.change(space, async (live) => {
			const actingAs = this.authorize(actor, live, 'people.manage');
			const person = this.personIn(live, did);
			const { role } = update;

			if (role !== undefined && !isRole(live.kind, role)) throw new UserError(`A ${live.kind} has no ${role} role`);

			const capabilities = update.capabilities
				? capabilitiesIn(live.kind, update.capabilities)
				: role !== undefined
					? capabilitiesFor(live.kind, role)
					: person.capabilities;

			const next = role ?? person.role;
			const updated: PersonRecord = { ...person, role: next, capabilities, updatedAt: nowIso() };
			const unmanaged = this.checkManagers(actor, live, new Map(live.people).set(did, updated));

			const changes: AuditChange[] = [
				{ field: 'role', before: person.role, after: next },
				{ field: 'capabilities', before: person.capabilities.join(' '), after: capabilities.join(' ') }
			].filter((change) => change.before !== change.after);

			await this.writePerson(live, updated);
			await this.record(
				actor,
				actingAs,
				'people.update',
				space,
				`Changed what ${await this.name(did)} can do`,
				{ person: did, role: next, capabilities, hidden: person.hidden, unmanaged },
				changes
			);
		});
	}

	/** Takes someone out of the space. Needs people.manage, and someone has to be left with it. */
	async removePerson(actor: Viewer, space: string, did: string): Promise<void> {
		await this.change(space, async (live) => {
			const actingAs = this.authorize(actor, live, 'people.manage');
			const person = this.personIn(live, did);
			const unmanaged = this.checkManagers(actor, live, this.without(live, did));

			await this.deletePerson(live, did);
			await this.record(actor, actingAs, 'people.remove', space, `Removed ${await this.name(did)}`, {
				person: did,
				role: person.role,
				capabilities: person.capabilities,
				hidden: person.hidden,
				unmanaged
			});
		});
	}

	/** `viewer` leaves the space, unless nobody else could manage its people then. */
	async leave(viewer: Viewer, space: string): Promise<void> {
		await this.change(space, async (live) => {
			const person = this.personIn(live, viewer.did);
			const unmanaged = this.checkManagers(viewer, live, this.without(live, viewer.did));

			await this.deletePerson(live, viewer.did);
			await this.record(viewer, unmanaged ? actorRole(viewer) : 'member', 'people.leave', space, 'Left', {
				person: viewer.did,
				role: person.role,
				capabilities: person.capabilities,
				hidden: person.hidden,
				unmanaged
			});
		});
	}

	/** Hides someone from everyone outside the space, or shows them again. Only they or staff can. */
	async setHidden(actor: Viewer, space: string, did: string, hidden: boolean): Promise<void> {
		if (actor.did !== did && !actor.isStaff) throw new ForbiddenError('Only they can change whether they show');

		await this.change(space, async (live) => {
			const person = this.personIn(live, did);

			if (person.hidden === hidden) return;
			await this.writePerson(live, { ...person, hidden, updatedAt: nowIso() });
			await this.record(
				actor,
				actor.did === did ? 'member' : actorRole(actor),
				'people.hide',
				space,
				hidden ? `Hid ${await this.name(did)}` : `Showed ${await this.name(did)}`,
				{ person: did, role: person.role, capabilities: person.capabilities, hidden }
			);
		});
	}

	/** Staff list the space in the directory, or take it out. */
	async setListed(actor: Viewer, space: string, listed: boolean): Promise<void> {
		if (!actor.isStaff) throw new ForbiddenError('Only staff can list it in the directory');

		await this.change(space, async (live) => {
			const stored = await live.root.getSpaceRecord({
				space,
				repo: live.root.did,
				collection: COLLECTION.listingStatus,
				rkey: LISTING_STATUS_KEY
			});

			const current = listingStatusRecord.safeParse(stored?.value);
			const now = nowIso();

			if (current.success && current.data.listed === listed) return;

			const status: ListingStatusRecord = {
				...(current.success ? current.data : { createdBy: actor.did, createdAt: now }),
				listed,
				decidedBy: actor.did,
				decidedAt: now
			};

			await live.root.putSpaceRecord({
				space,
				collection: COLLECTION.listingStatus,
				rkey: LISTING_STATUS_KEY,
				record: validateRecord(COLLECTION.listingStatus, status)
			});
			await this.saveStatus(space, live.kind, status);
			await this.record(
				actor,
				actorRole(actor),
				listed ? 'listing.list' : 'listing.unlist',
				space,
				listed ? 'Listed it in the directory' : 'Took it out of the directory',
				{}
			);
		});
	}

	/** Brings the space's member list in line with its people, as the root's repo has them now. */
	async syncMembers(space: string): Promise<void> {
		await this.change(space, async () => {});
	}

	/**
	 * Brings every shared space's member list in line with its people (run at startup and every hour), finishing a
	 * change whose member update failed part way.
	 */
	async reconcile(): Promise<void> {
		const root = await this.core.root();

		for (const type of Object.values(SHARED_SPACE_TYPES)) {
			let spaces: string[];

			try {
				spaces = await root.listSpaces(type);
			} catch (error) {
				this.core.logger.warn({ err: error, type }, "could not list the root's shared spaces");
				continue;
			}

			for (const space of spaces) {
				try {
					await this.syncMembers(space);
				} catch (error) {
					this.core.logger.warn({ err: error, space }, "could not sync a shared space's members");
				}
			}
		}
	}

	/**
	 * Indexes a record synced from a shared space, or one the root just wrote there. Callers check that the root wrote
	 * it, in a space under the root.
	 */
	async index(change: SpaceRecordChange): Promise<void> {
		const kind = sharedSpaceKind(parseSpaceUri(change.space)?.type ?? '');

		if (!kind) return;
		const indexed = await this.save(kind, change);

		if (!indexed)
			this.core.logger.warn(
				{ space: change.space, collection: change.collection, rkey: change.rkey },
				'ignoring a shared space record this app cannot read'
			);
	}

	/** Saves a shared space's record to the index, or returns false when it isn't one this app reads. */
	private async save(kind: SpaceKind, { space, collection, rkey, value }: SpaceRecordChange): Promise<boolean> {
		const { db } = this.core;

		switch (collection) {
			case COLLECTION.person: {
				const person = personRecord.safeParse(value);

				if (!person.success || person.data.subject !== rkey) return false;
				await this.savePerson(space, kind, person.data);

				return true;
			}

			case COLLECTION.invite: {
				const invite = inviteRecord.safeParse(value);

				if (!invite.success || invite.data.subject !== rkey) return false;
				const { role, capabilities, invitedBy, message, createdAt, expiresAt } = invite.data;

				const row = {
					role,
					capabilities: JSON.stringify(capabilitiesIn(kind, capabilities)),
					invitedBy,
					message: message ?? null,
					createdAt,
					expiresAt: expiresAt ?? null
				};

				await db
					.insertInto('spaceInvite')
					.values({ spaceUri: space, did: rkey, ...row })
					.onConflict((oc) => oc.columns(['spaceUri', 'did']).doUpdateSet(row))
					.execute();

				return true;
			}

			case COLLECTION.inviteLink: {
				const link = inviteLinkRecord.safeParse(value);

				if (!link.success) return false;
				const { tokenHash, role, capabilities, createdBy, createdAt, expiresAt } = link.data;

				const row = {
					tokenHash,
					role,
					capabilities: JSON.stringify(capabilitiesIn(kind, capabilities)),
					createdBy,
					createdAt,
					expiresAt: expiresAt ?? null
				};

				await db
					.insertInto('spaceInviteLink')
					.values({ spaceUri: space, rkey, ...row })
					.onConflict((oc) => oc.columns(['spaceUri', 'rkey']).doUpdateSet(row))
					.execute();

				return true;
			}

			case COLLECTION.joinRequest: {
				const request = joinRequestRecord.safeParse(value);

				if (!request.success || request.data.subject !== rkey) return false;
				const row = { message: request.data.message ?? null, createdAt: request.data.createdAt };

				await db
					.insertInto('spaceJoinRequest')
					.values({ spaceUri: space, did: rkey, ...row })
					.onConflict((oc) => oc.columns(['spaceUri', 'did']).doUpdateSet(row))
					.execute();

				return true;
			}

			case COLLECTION.listingStatus: {
				const status = listingStatusRecord.safeParse(value);

				if (!status.success || rkey !== LISTING_STATUS_KEY) return false;
				await this.saveStatus(space, kind, status.data);

				return true;
			}

			default:
				return false;
		}
	}

	/** Drops a record deleted from a shared space from the index. */
	async unindex(change: Omit<SpaceRecordChange, 'value'>): Promise<void> {
		const { db } = this.core;
		const { space, collection, rkey } = change;

		if (collection === COLLECTION.person)
			await db.deleteFrom('spacePerson').where('spaceUri', '=', space).where('did', '=', rkey).execute();
		else if (collection === COLLECTION.invite)
			await db.deleteFrom('spaceInvite').where('spaceUri', '=', space).where('did', '=', rkey).execute();
		else if (collection === COLLECTION.inviteLink)
			await db.deleteFrom('spaceInviteLink').where('spaceUri', '=', space).where('rkey', '=', rkey).execute();
		else if (collection === COLLECTION.joinRequest)
			await db.deleteFrom('spaceJoinRequest').where('spaceUri', '=', space).where('did', '=', rkey).execute();
		else if (collection === COLLECTION.listingStatus && rkey === LISTING_STATUS_KEY)
			await db.deleteFrom('sharedSpace').where('spaceUri', '=', space).execute();
	}

	/** Forgets a space that's gone. */
	async forget(space: string): Promise<void> {
		for (const table of ['spacePerson', 'spaceInvite', 'spaceInviteLink', 'spaceJoinRequest', 'sharedSpace'] as const)
			await this.core.db.deleteFrom(table).where('spaceUri', '=', space).execute();
	}

	/**
	 * Runs `task` in the space's lock with its people as the root's repo has them now, then brings the space's members in
	 * line with the people the task left.
	 */
	private async change(space: string, task: (live: Live) => Promise<void>): Promise<void> {
		const previous = this.locks.get(space);

		const run = (async () => {
			await previous;
			const live = await this.live(space);
			await task(live);
			await this.syncPeople(live);
		})();

		const settled = run.catch(() => undefined);
		this.locks.set(space, settled);

		try {
			await run;
		} finally {
			if (this.locks.get(space) === settled) this.locks.delete(space);
		}
	}

	/** The space's people as the root's own repo has them, since the index can be behind. */
	private async live(space: string): Promise<Live> {
		const ref = parseSpaceUri(space);
		const kind = ref?.authority === this.core.rootDid ? sharedSpaceKind(ref.type) : null;

		if (!kind) throw new NotFoundError('That space does not exist');
		const root = await this.core.root();
		const people = new Map<string, PersonRecord>();
		let cursor: string | undefined;

		do {
			const page = await root.listSpaceRecords({ space, repo: root.did, collection: COLLECTION.person, cursor });

			for (const { rkey, value } of page.records) {
				const person = personRecord.safeParse(value);

				if (person.success && person.data.subject === rkey) people.set(rkey, person.data);
			}

			cursor = page.cursor;
		} while (cursor);

		return { space, kind, root, people };
	}

	/** The actor role to audit `actor`'s change with, when they hold `capability` in the space or are staff. */
	private authorize(actor: Viewer, live: Live, capability: Capability): ActorRole {
		if (hasCapability(live.kind, live.people.get(actor.did), capability)) return 'member';

		if (actor.isStaff) return actorRole(actor);
		throw new ForbiddenError();
	}

	private personIn(live: Live, did: string): PersonRecord {
		const person = live.people.get(did);

		if (!person) throw new NotFoundError("They're not in it");

		return person;
	}

	private without(live: Live, did: string): Map<string, PersonRecord> {
		const people = new Map(live.people);
		people.delete(did);

		return people;
	}

	/**
	 * Refuses a change that would leave nobody able to manage the space's people, unless staff make it. Returns whether
	 * it does, so the audit entry notes it.
	 */
	private checkManagers(actor: Viewer, live: Live, after: Map<string, PersonRecord>): boolean {
		const manages = (people: Map<string, PersonRecord>) =>
			[...people.values()].some((person) => hasCapability(live.kind, person, 'people.manage'));

		if (manages(after) || !manages(live.people)) return false;

		if (!actor.isStaff) throw new UserError('Someone else has to be able to manage its people first');
		this.core.logger.warn({ space: live.space, actor: actor.did }, 'staff left a shared space nobody can manage');

		return true;
	}

	private async writePerson(live: Live, person: PersonRecord): Promise<void> {
		await live.root.putSpaceRecord({
			space: live.space,
			collection: COLLECTION.person,
			rkey: person.subject,
			record: validateRecord(COLLECTION.person, person)
		});
		live.people.set(person.subject, person);
		await this.savePerson(live.space, live.kind, person);
	}

	private async deletePerson(live: Live, did: string): Promise<void> {
		await live.root.deleteSpaceRecord({ space: live.space, collection: COLLECTION.person, rkey: did });
		live.people.delete(did);
		await this.unindex({ space: live.space, collection: COLLECTION.person, rkey: did });
	}

	/** Everyone in the space reads it; those with a capability that writes also write. */
	private async syncPeople(live: Live): Promise<void> {
		const wanted = new Map(
			[...live.people.values()].map((person) => [
				person.subject,
				{ read: true, write: isWriter(capabilitiesIn(live.kind, person.capabilities)) }
			])
		);

		await reconcileMembers(this.core, { uri: live.space, wanted, others: [], joinable: true });
	}

	private async savePerson(space: string, kind: SpaceKind, person: PersonRecord): Promise<void> {
		const row = {
			role: person.role,
			capabilities: JSON.stringify(capabilitiesIn(kind, person.capabilities)),
			hidden: person.hidden ? 1 : 0,
			via: person.via,
			addedBy: person.addedBy ?? null,
			source: person.source ?? null,
			createdAt: person.createdAt,
			updatedAt: person.updatedAt
		};

		await this.core.db
			.insertInto('spacePerson')
			.values({ spaceUri: space, did: person.subject, ...row })
			.onConflict((oc) => oc.columns(['spaceUri', 'did']).doUpdateSet(row))
			.execute();
	}

	private async saveStatus(space: string, kind: SpaceKind, status: ListingStatusRecord): Promise<void> {
		const row = { kind, listed: status.listed ? 1 : 0, createdBy: status.createdBy, createdAt: status.createdAt };

		await this.core.db
			.insertInto('sharedSpace')
			.values({ spaceUri: space, ...row })
			.onConflict((oc) => oc.column('spaceUri').doUpdateSet(row))
			.execute();
	}

	private async name(did: string): Promise<string> {
		const { handle } = await this.core.accounts.view(did);

		return handle ? `@${handle}` : did;
	}

	private async record(
		actor: Viewer,
		role: ActorRole,
		action: AuditAction,
		space: string,
		summary: string,
		details: Record<string, string | boolean | string[]>,
		changes?: AuditChange[]
	): Promise<void> {
		await this.core.audit.record({
			actor: actor.did,
			actorRole: role,
			action,
			subject: space,
			subjectKind: 'space',
			summary,
			changes,
			details
		});
	}
}
