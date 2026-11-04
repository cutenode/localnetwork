import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { spaceRecordUri } from '../atproto/spaces';
import { ForbiddenError } from '../core';
import { createTestServices } from '../testing/core';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

describe('StaffService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, DJ] });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const signIn = (did: string, handle: string) =>
		t.accounts.recordLogin({
			did,
			handle,
			pdsUrl: null,
			grantedScope: 'atproto',
			spacesCapable: true,
			publicWrites: true
		});

	/** Indexes a moderator role for `subject` as a sync of the staff space would, without touching any membership. */
	const synced = (subject: string) =>
		t.staff.indexRole(spaceRecordUri(t.spaces.staff, t.core.rootDid, COLLECTION.staffRole, subject), 'bafyrole', {
			subject,
			role: 'moderator',
			grantedBy: ADMIN,
			createdAt: '2026-10-01T00:00:00.000Z'
		});

	const members = (space: string) => [...(t.net.spaces.get(space)?.members.keys() ?? [])];

	it('names staff by handle in the audit log, with the note a grant came with', async () => {
		const admin = await t.viewer(ADMIN);
		await signIn(DJ, 'dj.test');

		await t.staff.grant(admin, DJ, 'admin', 'Runs the night');
		expect(await t.staff.list()).toMatchObject([
			{ did: ADMIN, bootstrap: true },
			{ did: DJ, handle: 'dj.test', role: 'admin', note: 'Runs the night', grantedBy: ADMIN, bootstrap: false }
		]);
		await t.staff.revoke(admin, DJ);

		await t.audit.idle();
		expect((await t.audit.list({ action: 'role' })).map((entry) => entry.summary)).toEqual([
			'Removed @dj.test from staff',
			'Made @dj.test an admin'
		]);
	});

	it('refuses grants and removals that only an admin, or nobody, may make', async () => {
		const admin = await t.viewer(ADMIN);
		await t.staff.grant(admin, MOD, 'moderator');
		const mod = await t.viewer(MOD);

		await expect(t.staff.grant(admin, 'dj.test', 'moderator')).rejects.toThrow('Enter a handle or DID');
		await expect(t.staff.revoke(mod, DJ)).rejects.toThrow(ForbiddenError);
		await t.staff.grant(admin, DJ, 'admin');
		await expect(t.staff.revoke(await t.viewer(DJ), DJ)).rejects.toThrow("You can't remove your own role");
		expect(await t.staff.roles(DJ)).toEqual(['admin']);
	});

	it('lists a configured admin once even when a role record names them too', async () => {
		await synced(ADMIN);

		expect(await t.staff.list()).toMatchObject([{ did: ADMIN, role: 'admin', bootstrap: true }]);
	});

	it('keeps adding a member to the other spaces when one of them fails', async () => {
		t.net.failWrites(t.core.rootDid);

		await t.staff.applyMembership(DJ, true);

		expect(members(t.spaces.audit)).not.toContain(DJ);
		expect(members(t.spaces.moderation)).toContain(DJ);
		expect(members(t.spaces.industry)).toContain(DJ);
	});

	it('puts every staff member, configured or synced, back in the spaces they should be in', async () => {
		await synced(MOD);
		await t.staff.applyMembership(ADMIN, false);
		expect(members(t.spaces.staff)).toEqual([]);

		await t.staff.reconcileMembership();

		for (const space of [t.spaces.audit, t.spaces.moderation, t.spaces.staff, t.spaces.industry])
			expect(members(space).toSorted()).toEqual([ADMIN, MOD]);
		expect(t.net.spaces.get(t.spaces.staff)?.members.get(MOD)).toEqual({ read: true, write: false });
	});
});
