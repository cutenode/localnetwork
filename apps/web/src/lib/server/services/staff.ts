import { z } from 'zod';
import { COLLECTION } from '#lib/nsid.ts';
import type { StaffGrant, StaffMember, StaffRole, Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { listAllSpaceRecords } from '../atproto/repo-client';
import { reconcileMembers } from './space-members';
import { type Core, ForbiddenError, UserError } from '../core';
import { nowIso, parseJson } from '../db';

const STAFF_GRANTS = ['flags', 'flagGroups'] as const satisfies readonly StaffGrant[];

const isStaffGrant = (value: string): value is StaffGrant => STAFF_GRANTS.some((grant) => grant === value);

/**
 * A computer.localnetwork.staff.role record. The lexicon leaves `role` open, so synced ones are
 * parsed with this to drop roles this app doesn't know.
 */
export const staffRoleRecord = z.object({
	subject: z.string(),
	role: z.enum(['admin', 'moderator'] as const satisfies readonly StaffRole[]),
	// Open in the lexicon too: a grant this app doesn't know gives nothing.
	grants: z
		.array(z.string())
		.transform((grants) => grants.filter(isStaffGrant))
		.optional(),
	grantedBy: z.string(),
	note: z.string().optional(),
	createdAt: z.string()
});

type StaffRoleRecord = z.infer<typeof staffRoleRecord>;

type StaffSpace = keyof Core['spaces'];

/** Access each staff member gets in the root's private spaces. */
const STAFF_ACCESS: { space: StaffSpace; read: boolean; write: boolean }[] = [
	{ space: 'audit', read: true, write: true },
	{ space: 'moderation', read: true, write: true },
	{ space: 'staff', read: true, write: false },
	{ space: 'industry', read: true, write: false }
];

const spaceNames = (spaces: StaffSpace[]) =>
	`the ${new Intl.ListFormat('en-GB', { type: 'conjunction' }).format(spaces)} space${spaces.length > 1 ? 's' : ''}`;

export class StaffService {
	private staffChanges: Promise<unknown> = Promise.resolve();

	constructor(private readonly core: Core) {}

	/** Runs one change to who's staff at a time, so a reconcile never acts on a list a grant or removal just changed. */
	private oneAtATime<T>(change: () => Promise<T>): Promise<T> {
		const run = this.staffChanges.then(change);
		this.staffChanges = run.catch(() => undefined);

		return run;
	}

	async roles(did: string): Promise<StaffRole[]> {
		if (this.core.bootstrapAdmins.includes(did)) return ['admin'];
		const row = await this.core.db.selectFrom('staffRole').select('role').where('did', '=', did).executeTakeFirst();

		return row ? [row.role] : [];
	}

	/** What `did` can do beyond its role. Admins can do all of it, and only staff have any. */
	async grants(did: string): Promise<StaffGrant[]> {
		if (this.core.bootstrapAdmins.includes(did)) return [...STAFF_GRANTS];

		const row = await this.core.db
			.selectFrom('staffRole')
			.select(['role', 'grants'])
			.where('did', '=', did)
			.executeTakeFirst();

		if (row?.role === 'admin') return [...STAFF_GRANTS];

		return parseJson<StaffGrant[]>(row?.grants, []);
	}

	async list(): Promise<StaffMember[]> {
		const rows = await this.core.db.selectFrom('staffRole').selectAll().orderBy('createdAt').execute();
		const dids = [...this.core.bootstrapAdmins, ...rows.map((row) => row.did)];
		const accounts = await this.core.accounts.views(dids);

		const members: StaffMember[] = this.core.bootstrapAdmins.map((did) => ({
			did,
			handle: accounts.get(did).handle,
			role: 'admin',
			grants: [...STAFF_GRANTS],
			grantedBy: null,
			note: 'Configured in ADMIN_DIDS',
			createdAt: null,
			bootstrap: true
		}));

		for (const row of rows) {
			if (this.core.bootstrapAdmins.includes(row.did)) continue;
			members.push({
				...row,
				grants: row.role === 'admin' ? [...STAFF_GRANTS] : parseJson<StaffGrant[]>(row.grants, []),
				handle: accounts.get(row.did).handle,
				bootstrap: false
			});
		}

		return members;
	}

	async grant(actor: Viewer, subject: string, role: StaffRole, note?: string): Promise<void> {
		if (!actor.isAdmin) throw new ForbiddenError('Only admins can manage staff');

		if (!subject.startsWith('did:')) throw new UserError('Enter a handle or DID');

		if (this.core.bootstrapAdmins.includes(subject)) throw new UserError('That account is a configured admin');

		const previous = await this.core.db
			.selectFrom('staffRole')
			.selectAll()
			.where('did', '=', subject)
			.executeTakeFirst();

		const root = await this.core.root();
		const grant: StaffRoleRecord = { subject, role, grantedBy: actor.did, createdAt: nowIso() };
		const grants = parseJson<StaffGrant[]>(previous?.grants, []);

		// Changing a moderator's note keeps what they were given. An admin has it all anyway.
		if (role === 'moderator' && grants.length > 0) grant.grants = grants;

		if (note) grant.note = note;
		const record = validateRecord(COLLECTION.staffRole, grant);

		await this.oneAtATime(async () => {
			const ref = await root.putSpaceRecord({
				space: this.core.spaces.staff,
				collection: COLLECTION.staffRole,
				rkey: subject,
				record
			});

			await this.indexRole(ref.uri, ref.cid, record);
			await this.applyMembership(subject, true);
		});

		const account = await this.core.accounts.view(subject);
		await this.core.audit.record({
			actor: actor.did,
			actorRole: 'admin',
			action: 'role.grant',
			subject,
			subjectKind: 'role',
			summary: `Made ${account.handle ? `@${account.handle}` : subject} ${role === 'admin' ? 'an admin' : 'a moderator'}`,
			changes: [{ field: 'role', before: previous?.role, after: role }],
			details: { note }
		});
	}

	/** Sets what a moderator can do beyond their role. */
	async setGrants(actor: Viewer, subject: string, grants: StaffGrant[]): Promise<void> {
		if (!actor.isAdmin) throw new ForbiddenError('Only admins can manage staff');

		const row = await this.core.db.selectFrom('staffRole').selectAll().where('did', '=', subject).executeTakeFirst();

		if (row?.role !== 'moderator') throw new UserError('Only moderators take grants');
		const previous = parseJson<StaffGrant[]>(row.grants, []);

		const value: StaffRoleRecord = {
			subject,
			role: row.role,
			grants,
			grantedBy: row.grantedBy ?? actor.did,
			createdAt: row.createdAt
		};

		if (row.note) value.note = row.note;
		const record = validateRecord(COLLECTION.staffRole, value);
		const root = await this.core.root();

		const ref = await root.putSpaceRecord({
			space: this.core.spaces.staff,
			collection: COLLECTION.staffRole,
			rkey: subject,
			record
		});

		await this.indexRole(ref.uri, ref.cid, record);
		const account = await this.core.accounts.view(subject);
		await this.core.audit.record({
			actor: actor.did,
			actorRole: 'admin',
			action: 'role.grant',
			subject,
			subjectKind: 'role',
			summary: `Changed what ${account.handle ? `@${account.handle}` : subject} can manage`,
			changes: [{ field: 'grants', before: previous.join(', '), after: grants.join(', ') }]
		});
	}

	async revoke(actor: Viewer, subject: string): Promise<void> {
		if (!actor.isAdmin) throw new ForbiddenError('Only admins can manage staff');

		if (this.core.bootstrapAdmins.includes(subject))
			throw new UserError('Configured admins are managed with ADMIN_DIDS');

		if (subject === actor.did) throw new UserError("You can't remove your own role");

		const previous = await this.core.db
			.selectFrom('staffRole')
			.selectAll()
			.where('did', '=', subject)
			.executeTakeFirst();

		if (!previous) throw new UserError('That account is not staff');
		const root = await this.core.root();

		// The role goes first, so the app stops trusting them whatever the spaces do. reconcileMembership finishes the rest.
		const stillIn = await this.oneAtATime(async () => {
			await root.deleteSpaceRecord({ space: this.core.spaces.staff, collection: COLLECTION.staffRole, rkey: subject });
			await this.core.db.deleteFrom('staffRole').where('did', '=', subject).execute();

			return this.applyMembership(subject, false);
		});

		const account = await this.core.accounts.view(subject);
		const who = account.handle ? `@${account.handle}` : subject;
		await this.core.audit.record({
			actor: actor.did,
			actorRole: 'admin',
			action: 'role.revoke',
			subject,
			subjectKind: 'role',
			summary:
				stillIn.length > 0
					? `Removed ${who} from staff, but they can still read ${spaceNames(stillIn)}`
					: `Removed ${who} from staff`,
			changes: [{ field: 'role', before: previous.role }],
			details: { stillIn },
			outcome: stillIn.length > 0 ? 'failure' : 'success'
		});
	}

	async indexRole(uri: string, cid: string, record: StaffRoleRecord) {
		const values = {
			role: record.role,
			grants: record.grants ? JSON.stringify(record.grants) : null,
			grantedBy: record.grantedBy,
			note: record.note ?? null,
			uri,
			cid,
			createdAt: record.createdAt
		};

		await this.core.db
			.insertInto('staffRole')
			.values({ did: record.subject, ...values })
			.onConflict((oc) => oc.column('did').doUpdateSet(values))
			.execute();
	}

	async removeRoleByUri(uri: string) {
		await this.core.db.deleteFrom('staffRole').where('uri', '=', uri).execute();
	}

	/**
	 * Adds or removes memberspace membership so staff can read the private spaces from any client. Returns the spaces it
	 * couldn't change.
	 */
	async applyMembership(did: string, isStaff: boolean): Promise<StaffSpace[]> {
		const root = await this.core.root();
		const failed = new Set(STAFF_ACCESS.map((access) => access.space));

		for (const access of STAFF_ACCESS) {
			const space = this.core.spaces[access.space];

			try {
				if (isStaff) await root.putMember({ space, did, read: access.read, write: access.write });
				else await root.removeMember({ space, did });
				failed.delete(access.space);
			} catch (error) {
				this.core.logger.warn('could not update space membership', { err: error, space, did });
			}
		}

		return [...failed];
	}

	/**
	 * Everyone who should be in the private spaces: anyone the app treats as staff, and anyone with a role in the root's
	 * repo, which the index can lag or, after a lost database, not have at all. Null when the root's roles can't be read,
	 * since nobody can then tell a former member from one the index hasn't heard of.
	 */
	private async expectedStaff(): Promise<Set<string> | null> {
		const { core } = this;
		const root = await core.root();

		try {
			const records = await listAllSpaceRecords(root, {
				space: core.spaces.staff,
				repo: root.did,
				collection: COLLECTION.staffRole
			});

			const indexed = await core.db.selectFrom('staffRole').select('did').execute();

			return new Set([
				...core.bootstrapAdmins,
				...indexed.map((row) => row.did),
				...records.flatMap(({ value }) => staffRoleRecord.safeParse(value).data?.subject ?? [])
			]);
		} catch (error) {
			core.logger.warn("could not read the root's staff roles", { err: error });

			return null;
		}
	}

	/**
	 * Makes the private spaces' member lists match the staff: adds who's missing, corrects access, and removes everyone
	 * else, which finishes any removal that failed. Each removal is audited here, so the log says when someone's access
	 * really went.
	 */
	reconcileMembership(): Promise<void> {
		return this.oneAtATime(async () => {
			const staff = await this.expectedStaff();

			if (!staff) return;
			const left = new Map<string, StaffSpace[]>();

			for (const { space, read, write } of STAFF_ACCESS) {
				await reconcileMembers(this.core, {
					uri: this.core.spaces[space],
					wanted: new Map([...staff].map((did) => [did, { read, write }])),
					others: [],
					joinable: true,
					onLeft: (did) => left.set(did, [...(left.get(did) ?? []), space])
				});
			}

			for (const [did, spaces] of left) {
				const { handle } = await this.core.accounts.view(did);
				await this.core.audit.record({
					actor: this.core.rootDid,
					actorRole: 'system',
					action: 'role.revoke',
					subject: did,
					subjectKind: 'role',
					summary: `Took ${handle ? `@${handle}` : did} out of ${spaceNames(spaces)}`,
					via: 'system',
					details: { spaces }
				});
			}
		});
	}

	/**
	 * Who's in a private space without being staff, read from the spaces themselves: a removal that hasn't gone through
	 * yet. Null when that can't be read right now.
	 */
	async strayMembers() {
		const { core } = this;
		const staff = await this.expectedStaff();

		if (!staff) return null;
		const root = await core.root();
		const strays = new Map<string, StaffSpace[]>();

		try {
			for (const { space } of STAFF_ACCESS) {
				for (const { did } of await root.listMembers(core.spaces[space])) {
					if (did !== root.did && !staff.has(did)) strays.set(did, [...(strays.get(did) ?? []), space]);
				}
			}
		} catch (error) {
			core.logger.warn("could not read the private spaces' members", { err: error });

			return null;
		}

		const accounts = await core.accounts.views([...strays.keys()]);

		return [...strays].map(([did, spaces]) => ({ did, handle: accounts.get(did).handle, spaces }));
	}
}
