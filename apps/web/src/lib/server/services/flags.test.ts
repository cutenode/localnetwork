import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { spaceRecordUri } from '../atproto/spaces';
import { ForbiddenError, UserError } from '../core';
import { createTestServices } from '../testing/core';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

const OTHER = 'did:plc:other';

const FLAG = 'parityCanary';

const off = { everyone: false, percentage: 0, groups: [], dids: [] };

describe('FlagService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, DJ] });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const stored = (collection: string, rkey: string) =>
		t.net.spaceRecords.get(`${t.spaces.staff}|${t.core.rootDid}|${collection}|${rkey}`);

	const moderator = async (...grants: ('flags' | 'flagGroups')[]) => {
		const admin = await t.viewer(ADMIN);
		await t.staff.grant(admin, MOD, 'moderator');

		if (grants.length > 0) await t.staff.setGrants(admin, MOD, grants);

		return t.viewer(MOD);
	};

	it('starts every flag off, for everyone', async () => {
		expect(await t.flags.evaluate(null)).toEqual({ [FLAG]: false });
		expect(await t.flags.evaluate(DJ)).toEqual({ [FLAG]: false });
		expect(await t.flags.answers(DJ)).toEqual([{ key: FLAG, everyone: false, percentage: 0, targeted: false }]);
	});

	it('reads the index again after failing to', async () => {
		vi.spyOn(t.db, 'selectFrom').mockImplementationOnce(() => {
			throw new Error('the disk went away');
		});

		await expect(t.flags.evaluate(null)).rejects.toThrow('the disk went away');
		expect(await t.flags.evaluate(null)).toEqual({ [FLAG]: false });
	});

	it('turns a flag on for one account and no one else', async () => {
		await t.flags.updateFlag(await t.viewer(ADMIN), FLAG, { ...off, dids: [DJ] });

		expect(await t.flags.evaluate(DJ)).toEqual({ [FLAG]: true });
		expect(await t.flags.evaluate(OTHER)).toEqual({ [FLAG]: false });
		expect(await t.flags.evaluate(null)).toEqual({ [FLAG]: false });
	});

	it('turns a flag on for a group, and follows its members', async () => {
		const admin = await t.viewer(ADMIN);
		const rkey = await t.flags.createGroup(admin, { name: 'Beta', members: [DJ] });
		await t.flags.updateFlag(admin, FLAG, { ...off, groups: [rkey] });

		expect(await t.flags.evaluate(DJ)).toEqual({ [FLAG]: true });
		expect(await t.flags.evaluate(OTHER)).toEqual({ [FLAG]: false });

		await t.flags.updateGroup(admin, rkey, { name: 'Beta', members: [OTHER] });

		expect(await t.flags.evaluate(DJ)).toEqual({ [FLAG]: false });
		expect(await t.flags.evaluate(OTHER)).toEqual({ [FLAG]: true });
	});

	it('shows signed-out visitors only what is on for everyone', async () => {
		const admin = await t.viewer(ADMIN);
		await t.flags.updateFlag(admin, FLAG, { ...off, percentage: 100, dids: [DJ] });

		expect(await t.flags.evaluate(DJ)).toEqual({ [FLAG]: true });
		expect(await t.flags.evaluate(null)).toEqual({ [FLAG]: false });

		await t.flags.updateFlag(admin, FLAG, { ...off, everyone: true });

		expect(await t.flags.evaluate(null)).toEqual({ [FLAG]: true });
	});

	it('tells a caller its own targeting and never who else', async () => {
		await t.flags.updateFlag(await t.viewer(ADMIN), FLAG, { ...off, percentage: 25, dids: [DJ] });

		expect(await t.flags.answers(DJ)).toEqual([{ key: FLAG, everyone: false, percentage: 25, targeted: true }]);
		expect(await t.flags.answers(OTHER)).toEqual([{ key: FLAG, everyone: false, percentage: 25, targeted: false }]);
		expect(await t.flags.answers(null)).toEqual([{ key: FLAG, everyone: false, percentage: 25, targeted: false }]);
	});

	it('writes to the staff space and logs what changed', async () => {
		await t.flags.updateFlag(await t.viewer(ADMIN), FLAG, { ...off, percentage: 10, dids: [DJ] });
		await t.audit.idle();

		expect(stored(COLLECTION.staffFlag, FLAG)?.value).toMatchObject({
			key: FLAG,
			percentage: 10,
			dids: [DJ],
			updatedBy: ADMIN
		});
		const [entry] = await t.audit.list({ action: 'flag.update' });

		expect(entry).toMatchObject({ actorDid: ADMIN, actorRole: 'admin', subject: FLAG, subjectKind: 'flag' });
		expect(JSON.parse(entry.changes ?? '')).toEqual([
			{ field: 'percentage', before: '0', after: '10' },
			{ field: 'accounts', before: '', after: DJ }
		]);
	});

	it('writes nothing when saving changes nothing', async () => {
		const admin = await t.viewer(ADMIN);
		await t.flags.updateFlag(admin, FLAG, { ...off, dids: [DJ] });
		await t.flags.updateFlag(admin, FLAG, { ...off, dids: [DJ] });
		await t.audit.idle();

		expect(await t.audit.list({ action: 'flag.update' })).toHaveLength(1);
	});

	it('refuses an unknown flag, a bad percentage and a group that is gone', async () => {
		const admin = await t.viewer(ADMIN);

		await expect(t.flags.updateFlag(admin, 'noSuchFlag', off)).rejects.toBeInstanceOf(UserError);
		await expect(t.flags.updateFlag(admin, FLAG, { ...off, percentage: 101 })).rejects.toThrow('0 to 100');
		await expect(t.flags.updateFlag(admin, FLAG, { ...off, percentage: 1.5 })).rejects.toThrow('0 to 100');
		await expect(t.flags.updateFlag(admin, FLAG, { ...off, groups: ['nope'] })).rejects.toThrow('no longer exists');
		expect(stored(COLLECTION.staffFlag, FLAG)).toBeUndefined();
	});

	it('lists a flag with its groups and accounts by name', async () => {
		const admin = await t.viewer(ADMIN);
		await t.accounts.recordLogin({
			did: DJ,
			handle: 'dj.test',
			pdsUrl: null,
			grantedScope: 'atproto',
			spacesCapable: true,
			publicWrites: true
		});
		const rkey = await t.flags.createGroup(admin, { name: 'Beta', description: 'Early', members: [DJ, OTHER] });
		await t.flags.updateFlag(admin, FLAG, { ...off, groups: [rkey], dids: [DJ] });

		expect(await t.flags.flag(FLAG)).toMatchObject({
			key: FLAG,
			groups: [{ rkey, name: 'Beta' }],
			accounts: [{ did: DJ, handle: 'dj.test' }]
		});
		expect(await t.flags.group(rkey)).toMatchObject({
			name: 'Beta',
			description: 'Early',
			members: [{ did: DJ, handle: 'dj.test' }, { did: OTHER }],
			flags: [FLAG]
		});
		expect(await t.flags.flag('noSuchFlag')).toBeNull();
		expect(await t.flags.group('nope')).toBeNull();
	});

	it('lists groups by name', async () => {
		const admin = await t.viewer(ADMIN);
		await t.flags.createGroup(admin, { name: 'Zines', members: [] });
		await t.flags.createGroup(admin, { name: 'Beta', members: [] });

		expect((await t.flags.groups()).map((group) => group.name)).toEqual(['Beta', 'Zines']);
	});

	it('refuses two groups with one name, whatever the case', async () => {
		const admin = await t.viewer(ADMIN);
		const beta = await t.flags.createGroup(admin, { name: 'Beta', members: [] });
		const other = await t.flags.createGroup(admin, { name: 'Other', members: [] });

		await expect(t.flags.createGroup(admin, { name: 'beta', members: [] })).rejects.toThrow('already a group');
		await expect(t.flags.updateGroup(admin, other, { name: 'BETA', members: [] })).rejects.toThrow('already a group');
		await expect(t.flags.updateGroup(admin, beta, { name: 'BETA', members: [] })).resolves.toBeUndefined();
	});

	it('renames a group without touching the flags that use it', async () => {
		const admin = await t.viewer(ADMIN);
		const rkey = await t.flags.createGroup(admin, { name: 'Beta', members: [DJ] });
		await t.flags.updateFlag(admin, FLAG, { ...off, groups: [rkey] });
		const before = stored(COLLECTION.staffFlag, FLAG)?.cid;
		await t.flags.updateGroup(admin, rkey, { name: 'Early', members: [DJ] });

		expect(stored(COLLECTION.staffFlag, FLAG)?.cid).toBe(before);
		expect(await t.flags.flag(FLAG)).toMatchObject({ groups: [{ rkey, name: 'Early' }] });
		expect(await t.flags.evaluate(DJ)).toEqual({ [FLAG]: true });
	});

	it("won't delete a group a flag uses, then does once it's free", async () => {
		const admin = await t.viewer(ADMIN);
		const rkey = await t.flags.createGroup(admin, { name: 'Beta', members: [DJ] });
		await t.flags.updateFlag(admin, FLAG, { ...off, groups: [rkey] });

		await expect(t.flags.deleteGroup(admin, rkey)).rejects.toThrow(`Take it off ${FLAG} first`);
		expect(await t.flags.evaluate(DJ)).toEqual({ [FLAG]: true });

		await t.flags.updateFlag(admin, FLAG, off);
		await t.flags.deleteGroup(admin, rkey);
		await t.audit.idle();

		expect(stored(COLLECTION.staffGroup, rkey)).toBeUndefined();
		expect(await t.flags.groups()).toEqual([]);
		expect((await t.audit.list({ action: 'flagGroup.delete' }))[0]).toMatchObject({
			subject: rkey,
			subjectKind: 'flagGroup'
		});
		await expect(t.flags.deleteGroup(admin, rkey)).rejects.toThrow('no longer exists');
		await expect(t.flags.updateGroup(admin, rkey, { name: 'Gone', members: [] })).rejects.toThrow('no longer exists');
	});

	it('takes a flag and a group from a sync, and forgets them when their records go', async () => {
		const flagUri = spaceRecordUri(t.spaces.staff, t.core.rootDid, COLLECTION.staffFlag, FLAG);
		const groupUri = spaceRecordUri(t.spaces.staff, t.core.rootDid, COLLECTION.staffGroup, '3lzy2ji4nms2z');
		const at = '2026-11-01T00:00:00.000Z';

		await t.flags.indexGroup(groupUri, { name: 'Beta', members: [DJ], createdAt: at, updatedBy: ADMIN, updatedAt: at });
		await t.flags.indexFlag(flagUri, {
			key: FLAG,
			everyone: false,
			percentage: 0,
			groups: ['3lzy2ji4nms2z'],
			dids: [],
			updatedBy: ADMIN,
			updatedAt: at
		});

		expect(await t.flags.evaluate(DJ)).toEqual({ [FLAG]: true });

		await t.flags.removeGroupByUri(groupUri);

		expect(await t.flags.evaluate(DJ)).toEqual({ [FLAG]: false });

		await t.flags.indexFlag(flagUri, {
			key: FLAG,
			everyone: true,
			percentage: 0,
			groups: [],
			dids: [],
			updatedBy: ADMIN,
			updatedAt: at
		});

		expect(await t.flags.evaluate(null)).toEqual({ [FLAG]: true });

		await t.flags.removeFlagByUri(flagUri);

		expect(await t.flags.evaluate(null)).toEqual({ [FLAG]: false });
	});

	it('ignores a record for a flag that has left the catalog', async () => {
		const at = '2026-11-01T00:00:00.000Z';
		const uri = spaceRecordUri(t.spaces.staff, t.core.rootDid, COLLECTION.staffFlag, 'retired');

		await t.flags.indexFlag(uri, {
			key: 'retired',
			everyone: true,
			percentage: 0,
			groups: [],
			dids: [],
			updatedBy: ADMIN,
			updatedAt: at
		});

		expect(Object.keys(await t.flags.evaluate(null))).toEqual([FLAG]);
		expect((await t.flags.list()).map((flag) => flag.key)).toEqual([FLAG]);
	});

	describe('who may change them', () => {
		it('lets a moderator do only what they were given', async () => {
			const onlyFlags = await moderator('flags');

			await expect(t.flags.updateFlag(onlyFlags, FLAG, { ...off, dids: [DJ] })).resolves.toBeUndefined();
			await expect(t.flags.createGroup(onlyFlags, { name: 'Beta', members: [] })).rejects.toBeInstanceOf(
				ForbiddenError
			);

			await t.staff.setGrants(await t.viewer(ADMIN), MOD, ['flagGroups']);
			const onlyGroups = await t.viewer(MOD);

			await expect(t.flags.createGroup(onlyGroups, { name: 'Beta', members: [] })).resolves.toEqual(expect.any(String));
			await expect(t.flags.updateFlag(onlyGroups, FLAG, off)).rejects.toBeInstanceOf(ForbiddenError);
			await t.audit.idle();

			expect((await t.audit.list({ action: 'flag.update' }))[0]).toMatchObject({
				actorDid: MOD,
				actorRole: 'moderator'
			});
		});

		it('turns away a moderator with no grants and anyone without a role', async () => {
			const bare = await moderator();
			const dj = await t.viewer(DJ);

			await expect(t.flags.updateFlag(bare, FLAG, off)).rejects.toBeInstanceOf(ForbiddenError);
			await expect(t.flags.updateFlag(dj, FLAG, off)).rejects.toBeInstanceOf(ForbiddenError);
			await expect(t.flags.createGroup(dj, { name: 'Beta', members: [] })).rejects.toBeInstanceOf(ForbiddenError);
			await expect(t.flags.updateGroup(dj, 'x', { name: 'Beta', members: [] })).rejects.toBeInstanceOf(ForbiddenError);
			await expect(t.flags.deleteGroup(dj, 'x')).rejects.toBeInstanceOf(ForbiddenError);
		});
	});
});
