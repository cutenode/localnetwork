import { z } from 'zod';
import { COLLECTION } from '#lib/nsid.ts';
import type { StaffMember, StaffRole, Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { type Core, ForbiddenError, UserError } from '../core';
import { nowIso } from '../db';

/**
 * A computer.localnetwork.staff.role record. The lexicon leaves `role` open, so synced ones are
 * parsed with this to drop roles this app doesn't know.
 */
export const staffRoleRecord = z.object({
	subject: z.string(),
	role: z.enum(['admin', 'moderator'] as const satisfies readonly StaffRole[]),
	grantedBy: z.string(),
	note: z.string().optional(),
	createdAt: z.string()
});

type StaffRoleRecord = z.infer<typeof staffRoleRecord>;

/** Access each staff member gets in the root's private spaces. */
const STAFF_ACCESS: { space: keyof Core['spaces']; read: boolean; write: boolean }[] = [
	{ space: 'audit', read: true, write: true },
	{ space: 'moderation', read: true, write: true },
	{ space: 'staff', read: true, write: false },
	{ space: 'industry', read: true, write: false }
];

export class StaffService {
	constructor(private readonly core: Core) {}

	async roles(did: string): Promise<StaffRole[]> {
		if (this.core.bootstrapAdmins.includes(did)) return ['admin'];
		const row = await this.core.db.selectFrom('staffRole').select('role').where('did', '=', did).executeTakeFirst();

		return row ? [row.role] : [];
	}

	async list(): Promise<StaffMember[]> {
		const rows = await this.core.db.selectFrom('staffRole').selectAll().orderBy('createdAt').execute();
		const dids = [...this.core.bootstrapAdmins, ...rows.map((row) => row.did)];
		const accounts = await this.core.accounts.views(dids);

		const members: StaffMember[] = this.core.bootstrapAdmins.map((did) => ({
			did,
			handle: accounts.get(did)?.handle ?? null,
			role: 'admin',
			grantedBy: null,
			note: 'Configured in ADMIN_DIDS',
			createdAt: null,
			bootstrap: true
		}));

		for (const row of rows) {
			if (this.core.bootstrapAdmins.includes(row.did)) continue;
			members.push({ ...row, handle: accounts.get(row.did)?.handle ?? null, bootstrap: false });
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

		if (note) grant.note = note;
		const record = validateRecord(COLLECTION.staffRole, grant);

		const ref = await root.putSpaceRecord({
			space: this.core.spaces.staff,
			collection: COLLECTION.staffRole,
			rkey: subject,
			record
		});

		await this.indexRole(ref.uri, ref.cid, record);
		await this.applyMembership(subject, true);
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
		await root.deleteSpaceRecord({ space: this.core.spaces.staff, collection: COLLECTION.staffRole, rkey: subject });
		await this.core.db.deleteFrom('staffRole').where('did', '=', subject).execute();
		await this.applyMembership(subject, false);
		const account = await this.core.accounts.view(subject);
		await this.core.audit.record({
			actor: actor.did,
			actorRole: 'admin',
			action: 'role.revoke',
			subject,
			subjectKind: 'role',
			summary: `Removed ${account.handle ? `@${account.handle}` : subject} from staff`,
			changes: [{ field: 'role', before: previous.role }]
		});
	}

	async indexRole(uri: string, cid: string, record: StaffRoleRecord) {
		const values = {
			role: record.role,
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

	/** Adds or removes memberspace membership so staff can read the private spaces from any client. */
	async applyMembership(did: string, isStaff: boolean): Promise<void> {
		const root = await this.core.root();

		for (const access of STAFF_ACCESS) {
			const space = this.core.spaces[access.space];

			try {
				if (isStaff) await root.putMember({ space, did, read: access.read, write: access.write });
				else await root.removeMember({ space, did });
			} catch (error) {
				this.core.logger.warn({ err: error, space, did }, 'could not update space membership');
			}
		}
	}

	/** Makes space membership match the current staff list (run at startup). */
	async reconcileMembership(): Promise<void> {
		const staff = new Set([...this.core.bootstrapAdmins, ...(await this.list()).map((member) => member.did)]);

		for (const did of staff) await this.applyMembership(did, true);
	}
}
