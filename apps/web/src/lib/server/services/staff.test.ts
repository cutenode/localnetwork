import { XRPCError } from '@atproto/api';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { spaceRecordUri } from '../atproto/spaces';
import { ForbiddenError } from '../core';
import { FakeRootClient } from '../testing/fakes';
import { createTestServices } from '../testing/core';
import { staffRoleRecord } from './staff';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

describe('StaffService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, DJ] });
	});
	afterEach(async () => {
		vi.restoreAllMocks();
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

	it('gives admins every grant and moderators only theirs', async () => {
		const admin = await t.viewer(ADMIN);

		expect(await t.staff.grants(ADMIN)).toEqual(['flags', 'flagGroups']);
		expect(await t.staff.grants(DJ)).toEqual([]);
		await t.staff.grant(admin, MOD, 'moderator');
		expect(await t.staff.grants(MOD)).toEqual([]);

		await t.staff.setGrants(admin, MOD, ['flags']);
		await t.audit.idle();

		expect(await t.staff.grants(MOD)).toEqual(['flags']);
		expect((await t.staff.list()).find((member) => member.did === MOD)?.grants).toEqual(['flags']);
		expect(JSON.parse((await t.audit.list({ action: 'role.grant' }))[0].changes ?? '')).toEqual([
			{ field: 'grants', before: '', after: 'flags' }
		]);

		await t.staff.grant(admin, MOD, 'admin');
		expect(await t.staff.grants(MOD)).toEqual(['flags', 'flagGroups']);
	});

	it("keeps a moderator's grants when their role is granted again", async () => {
		const admin = await t.viewer(ADMIN);
		await t.staff.grant(admin, MOD, 'moderator');
		await t.staff.setGrants(admin, MOD, ['flagGroups']);
		await t.staff.grant(admin, MOD, 'moderator', 'Weekends');

		expect(await t.staff.grants(MOD)).toEqual(['flagGroups']);
	});

	it('only lets admins set grants, and only on moderators', async () => {
		const admin = await t.viewer(ADMIN);
		await t.staff.grant(admin, MOD, 'moderator');

		await expect(t.staff.setGrants(await t.viewer(MOD), MOD, ['flags'])).rejects.toBeInstanceOf(ForbiddenError);
		await expect(t.staff.setGrants(admin, DJ, ['flags'])).rejects.toThrow('Only moderators take grants');
		expect(await t.staff.grants(MOD)).toEqual([]);
	});

	it("drops a grant it doesn't know from a synced role", () => {
		const parsed = staffRoleRecord.parse({
			subject: MOD,
			role: 'moderator',
			grants: ['flags', 'someday'],
			grantedBy: ADMIN,
			createdAt: '2026-10-01T00:00:00.000Z'
		});

		expect(parsed.grants).toEqual(['flags']);
	});

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

	describe('taking someone out of the private spaces', () => {
		const SPACES = ['audit', 'moderation', 'staff', 'industry'] as const;

		/** Makes the root's removals from `spaces` fail, as a PDS failing those calls would. */
		const failRemovalsFrom = (...spaces: string[]) =>
			vi.spyOn(FakeRootClient.prototype, 'removeMember').mockImplementation(async ({ space, did }) => {
				if (spaces.includes(space)) throw new XRPCError(502, 'UpstreamFailure');
				t.net.spaces.get(space)?.members.delete(did);
			});

		const roleRecord = (subject: string, role: string) =>
			t.net.client(t.core.rootDid).putSpaceRecord({
				space: t.spaces.staff,
				collection: COLLECTION.staffRole,
				rkey: subject,
				record: {
					$type: COLLECTION.staffRole,
					subject,
					role,
					grantedBy: ADMIN,
					createdAt: '2026-10-01T00:00:00.000Z'
				}
			});

		const spacesOf = (did: string) => SPACES.filter((space) => members(t.spaces[space]).includes(did));

		it('finishes a failed removal on the next reconcile that works', async () => {
			const admin = await t.viewer(ADMIN);
			await t.staff.grant(admin, MOD, 'moderator');
			const failing = failRemovalsFrom(t.spaces.audit, t.spaces.staff);

			await t.staff.revoke(admin, MOD);
			expect(members(t.spaces.audit)).toContain(MOD);
			expect(members(t.spaces.staff)).toContain(MOD);
			expect(members(t.spaces.moderation)).not.toContain(MOD);
			expect(await t.staff.roles(MOD)).toEqual([]);

			await t.staff.reconcileMembership();
			expect(members(t.spaces.audit)).toContain(MOD);

			failing.mockRestore();
			await t.staff.reconcileMembership();
			expect(spacesOf(MOD)).toEqual([]);
		});

		it("doesn't log a removal as done until the access is gone", async () => {
			const admin = await t.viewer(ADMIN);
			await signIn(MOD, 'mod.test');
			await t.staff.grant(admin, MOD, 'moderator');
			const failing = failRemovalsFrom(t.spaces.audit, t.spaces.staff);

			await t.staff.revoke(admin, MOD);
			await t.staff.reconcileMembership();
			failing.mockRestore();
			await t.staff.reconcileMembership();
			await t.staff.reconcileMembership();

			await t.audit.idle();
			const entries = await t.audit.list({ action: 'role.revoke' });

			expect(entries).toHaveLength(2);
			expect(entries[1]).toMatchObject({
				actorDid: ADMIN,
				outcome: 'failure',
				summary: 'Removed @mod.test from staff, but they can still read the audit and staff spaces'
			});
			expect(entries[0]).toMatchObject({
				actorDid: t.core.rootDid,
				actorRole: 'system',
				outcome: 'success',
				summary: 'Took @mod.test out of the audit and staff spaces'
			});
		});

		it('shows admins who is left in a space without a role, until they are gone', async () => {
			const admin = await t.viewer(ADMIN);
			await signIn(MOD, 'mod.test');
			await t.staff.grant(admin, MOD, 'moderator');
			expect(await t.staff.strayMembers()).toEqual([]);
			const failing = failRemovalsFrom(t.spaces.audit, t.spaces.industry);

			await t.staff.revoke(admin, MOD);
			expect(await t.staff.strayMembers()).toEqual([{ did: MOD, handle: 'mod.test', spaces: ['audit', 'industry'] }]);

			failing.mockRestore();
			await t.staff.reconcileMembership();
			expect(await t.staff.strayMembers()).toEqual([]);
		});

		it("doesn't guess who's left over when the private spaces can't be read", async () => {
			vi.spyOn(FakeRootClient.prototype, 'listMembers').mockRejectedValue(new XRPCError(502, 'UpstreamFailure'));

			await expect(t.staff.strayMembers()).resolves.toBeNull();
		});

		it('removes someone whose role went without a revoke, as a synced delete does', async () => {
			await t.staff.applyMembership(DJ, true);

			await t.staff.reconcileMembership();

			expect(spacesOf(DJ)).toEqual([]);
		});

		it('removes someone whose role record holds a role this app no longer knows', async () => {
			await t.staff.applyMembership(DJ, true);
			await roleRecord(DJ, 'owner');

			await t.staff.reconcileMembership();

			expect(spacesOf(DJ)).toEqual([]);
		});

		it('never removes a configured admin or the root', async () => {
			await t.staff.applyMembership(ADMIN, true);
			await t.net
				.client(t.core.rootDid)
				.putMember({ space: t.spaces.audit, did: t.core.rootDid, read: true, write: true });

			await t.staff.reconcileMembership();

			expect(members(t.spaces.audit).toSorted()).toEqual([ADMIN, t.core.rootDid].toSorted());
			expect(await t.staff.strayMembers()).toEqual([]);
		});

		it('goes by the root over an index a lost database has yet to rebuild', async () => {
			await roleRecord(MOD, 'moderator');
			await t.staff.applyMembership(MOD, true);
			expect(await t.staff.roles(MOD)).toEqual([]);

			await t.staff.reconcileMembership();

			expect(spacesOf(MOD)).toEqual(SPACES);
			expect(await t.staff.strayMembers()).toEqual([]);
		});

		it("removes nobody when the root's roles can't be read", async () => {
			await t.staff.applyMembership(DJ, true);
			vi.spyOn(FakeRootClient.prototype, 'listSpaceRecords').mockRejectedValue(new XRPCError(502, 'UpstreamFailure'));

			await t.staff.reconcileMembership();

			expect(spacesOf(DJ)).toEqual(SPACES);
			await expect(t.staff.strayMembers()).resolves.toBeNull();
		});

		it('carries on with the next change after one that failed', async () => {
			const admin = await t.viewer(ADMIN);
			t.net.failWrites(t.core.rootDid);

			await expect(t.staff.grant(admin, MOD, 'moderator')).rejects.toThrow(XRPCError);
			await t.staff.grant(admin, MOD, 'moderator');

			expect(await t.staff.roles(MOD)).toEqual(['moderator']);
		});

		it("can't put back someone removed while it ran, or remove someone added", async () => {
			const admin = await t.viewer(ADMIN);
			await t.staff.grant(admin, MOD, 'moderator');
			let resume = () => {};

			const paused = new Promise<void>((resolve) => {
				resume = resolve;
			});

			const listing = vi.spyOn(FakeRootClient.prototype, 'listMembers').mockImplementationOnce(async (space) => {
				await paused;

				return members(space).map((did) => ({ did, read: true, write: true }));
			});

			const reconcile = t.staff.reconcileMembership();
			await vi.waitFor(() => expect(listing).toHaveBeenCalled());
			const changes = Promise.all([t.staff.revoke(admin, MOD), t.staff.grant(admin, DJ, 'moderator')]);
			// Time for the changes to go ahead of it, if nothing holds them back.
			await new Promise((resolve) => setTimeout(resolve, 50));
			resume();
			await Promise.all([reconcile, changes]);

			expect(spacesOf(MOD)).toEqual([]);

			expect(spacesOf(DJ)).toEqual(SPACES);
		});
	});
});
