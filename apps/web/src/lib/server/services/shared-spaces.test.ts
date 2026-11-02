import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { memberSpacePolicy } from '../atproto/repo-client';
import { FakeRootClient } from '../testing/fakes';
import { createTestServices, ROOT } from '../testing/core';
import { ForbiddenError, NotFoundError, UserError } from '../core';

const ADMIN = 'did:plc:admin';

const ADA = 'did:plc:ada';

const BO = 'did:plc:bo';

const CY = 'did:plc:cy';

const readWrite = { read: true, write: true };

describe('SharedSpaces', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN] });
	});
	afterEach(async () => {
		vi.restoreAllMocks();
		await t.audit.idle();
		await t.db.destroy();
	});

	const access = (space: string) => Object.fromEntries(t.net.spaces.get(space)!.members);

	const person = (space: string, did: string) =>
		t.net.spaceRecords.get(`${space}|${ROOT}|${COLLECTION.person}|${did}`)?.value;

	const audited = async (action: string) =>
		(await t.audit.list({ action })).map(({ actorDid: actor, actorRole, subject, details }) => ({
			actor,
			actorRole,
			subject,
			details: JSON.parse(details ?? '{}')
		}));

	/** A party Ada starts, with Bo added by staff as `role`. */
	const party = async (role = 'cohost') => {
		const space = await t.sharedSpaces.create(await t.viewer(ADA), 'party');
		await t.sharedSpaces.addPerson(await t.viewer(ADMIN), space, { did: BO, role });

		return space;
	};

	it('starts a space only its people read and write, with its creator in it holding every capability of its kind', async () => {
		const space = await t.sharedSpaces.create(await t.viewer(ADA), 'performer');

		expect(space).toMatch(/^at:\/\/did:plc:root\/space\/computer\.localnetwork\.space\.performer\//);
		expect(t.net.spaces.get(space)?.policy).toEqual(memberSpacePolicy());
		expect(access(space)).toEqual({ [ADA]: readWrite });
		expect(person(space, ADA)).toMatchObject({ subject: ADA, role: 'member', hidden: false, via: 'created' });
		expect(await t.sharedSpaces.get(space)).toMatchObject({ kind: 'performer', listed: false, createdBy: ADA });

		const [ada] = await t.sharedSpaces.people(space, null);
		expect(ada).toMatchObject({ did: ADA, role: 'member', via: 'created' });
		expect(ada.capabilities).toEqual([
			'page.edit',
			'people.manage',
			'listing.request',
			'presskits.manage',
			'bookings.answer',
			'shows.manage'
		]);
		expect(await audited('space.create')).toEqual([
			{ actor: ADA, actorRole: 'member', subject: space, details: { person: ADA, role: 'member' } }
		]);
	});

	it("makes someone whose capabilities don't write a reader only", async () => {
		const space = await party('door');

		expect(access(space)).toEqual({ [ADA]: readWrite, [BO]: { read: true, write: false } });
		expect((await t.sharedSpaces.people(space, null)).map((view) => view.capabilities)[1]).toEqual(['guests.checkin']);
	});

	it('lets only staff add people directly, once each, in a role of the kind', async () => {
		const space = await party();
		const admin = await t.viewer(ADMIN);

		await expect(
			t.sharedSpaces.addPerson(await t.viewer(ADA), space, { did: CY, role: 'door' })
		).rejects.toBeInstanceOf(ForbiddenError);
		await expect(t.sharedSpaces.addPerson(admin, space, { did: BO, role: 'door' })).rejects.toThrow('already in it');
		await expect(t.sharedSpaces.addPerson(admin, space, { did: CY, role: 'member' })).rejects.toThrow(
			'A party has no member role'
		);
		expect(await audited('people.add')).toEqual([
			{
				actor: ADMIN,
				actorRole: 'admin',
				subject: space,
				details: {
					person: BO,
					role: 'cohost',
					capabilities: [
						'page.edit',
						'listing.request',
						'events.manage',
						'bookings.send',
						'guests.manage',
						'guests.checkin'
					],
					hidden: false
				}
			}
		]);
	});

	it('lets people.manage holders change and remove people, and nobody else', async () => {
		const space = await party();
		const bo = await t.viewer(BO);
		const ada = await t.viewer(ADA);

		await expect(t.sharedSpaces.updatePerson(bo, space, ADA, { role: 'door' })).rejects.toBeInstanceOf(ForbiddenError);
		await expect(t.sharedSpaces.removePerson(bo, space, ADA)).rejects.toBeInstanceOf(ForbiddenError);

		await t.sharedSpaces.updatePerson(ada, space, BO, { role: 'door' });
		expect(person(space, BO)).toMatchObject({ role: 'door', capabilities: ['guests.checkin'] });
		expect(access(space)[BO]).toEqual({ read: true, write: false });

		// Capabilities can differ from the role's; ones the kind doesn't offer are dropped.
		await t.sharedSpaces.updatePerson(ada, space, BO, {
			capabilities: ['events.manage', 'venue.block', 'guests.checkin']
		});
		expect(person(space, BO)).toMatchObject({ role: 'door', capabilities: ['events.manage', 'guests.checkin'] });
		expect(access(space)[BO]).toEqual(readWrite);
		await expect(t.sharedSpaces.updatePerson(ada, space, BO, { role: 'owner' })).rejects.toBeInstanceOf(UserError);

		const changes = (await t.audit.list({ action: 'people.update' })).map((entry) => JSON.parse(entry.changes ?? '[]'));
		expect(changes).toEqual([
			[{ field: 'capabilities', before: 'guests.checkin', after: 'events.manage guests.checkin' }],
			[
				{ field: 'role', before: 'cohost', after: 'door' },
				{
					field: 'capabilities',
					before: 'page.edit listing.request events.manage bookings.send guests.manage guests.checkin',
					after: 'guests.checkin'
				}
			]
		]);

		await t.sharedSpaces.removePerson(ada, space, BO);
		expect(person(space, BO)).toBeUndefined();
		expect(access(space)).toEqual({ [ADA]: readWrite });
		expect((await t.sharedSpaces.people(space, null)).map((view) => view.did)).toEqual([ADA]);
		await expect(t.sharedSpaces.removePerson(ada, space, BO)).rejects.toBeInstanceOf(NotFoundError);
	});

	it('keeps someone able to manage its people, unless staff decide otherwise', async () => {
		const space = await party();
		const ada = await t.viewer(ADA);

		await expect(t.sharedSpaces.leave(ada, space)).rejects.toThrow('Someone else has to be able to manage');
		await expect(t.sharedSpaces.updatePerson(ada, space, ADA, { role: 'cohost' })).rejects.toBeInstanceOf(UserError);
		await expect(t.sharedSpaces.removePerson(ada, space, ADA)).rejects.toBeInstanceOf(UserError);
		expect(person(space, ADA)).toMatchObject({ role: 'host' });

		// Once Bo can manage its people too, Ada can go.
		await t.sharedSpaces.updatePerson(ada, space, BO, { role: 'host' });
		await t.sharedSpaces.leave(ada, space);
		expect(access(space)).toEqual({ [BO]: readWrite });
		expect(await audited('people.leave')).toMatchObject([
			{ actor: ADA, actorRole: 'member', details: { unmanaged: false } }
		]);

		await t.sharedSpaces.removePerson(await t.viewer(ADMIN), space, BO);
		expect(access(space)).toEqual({});
		expect(await audited('people.remove')).toMatchObject([
			{ actor: ADMIN, actorRole: 'admin', details: { person: BO, role: 'host', unmanaged: true } }
		]);
	});

	it('lets only one of two concurrent removals take away the last two who manage its people', async () => {
		const space = await party('host');
		const [ada, bo] = [await t.viewer(ADA), await t.viewer(BO)];

		const results = await Promise.allSettled([
			t.sharedSpaces.removePerson(ada, space, BO),
			t.sharedSpaces.removePerson(bo, space, ADA)
		]);

		expect(results.map((result) => result.status)).toEqual(['fulfilled', 'rejected']);
		expect(access(space)).toEqual({ [ADA]: readWrite });
	});

	it('lets only the person or staff hide them, and shows hidden people only inside the space and to staff', async () => {
		const space = await party();

		await expect(t.sharedSpaces.setHidden(await t.viewer(ADA), space, BO, true)).rejects.toBeInstanceOf(ForbiddenError);
		await t.sharedSpaces.setHidden(await t.viewer(BO), space, BO, true);
		expect(person(space, BO)).toMatchObject({ hidden: true });

		const seen = async (did: string | null) =>
			(await t.sharedSpaces.people(space, did ? await t.viewer(did) : null)).map((view) => view.did);

		expect(await seen(null)).toEqual([ADA]);
		expect(await seen(CY)).toEqual([ADA]);
		expect(await seen(ADA)).toEqual([ADA, BO]);
		expect(await seen(ADMIN)).toEqual([ADA, BO]);

		await t.sharedSpaces.setHidden(await t.viewer(ADMIN), space, BO, false);
		expect(await seen(null)).toEqual([ADA, BO]);
		expect(await audited('people.hide')).toMatchObject([
			{ actor: ADMIN, actorRole: 'admin', details: { person: BO, hidden: false } },
			{ actor: BO, actorRole: 'member', details: { person: BO, hidden: true } }
		]);
	});

	it('lets only staff list it in the directory, keeping who created it', async () => {
		const space = await party();
		const admin = await t.viewer(ADMIN);

		await expect(t.sharedSpaces.setListed(await t.viewer(ADA), space, true)).rejects.toBeInstanceOf(ForbiddenError);
		await t.sharedSpaces.setListed(admin, space, true);
		await t.sharedSpaces.setListed(admin, space, true);

		expect(await t.sharedSpaces.get(space)).toMatchObject({ listed: true, createdBy: ADA });
		expect(t.net.spaceRecords.get(`${space}|${ROOT}|${COLLECTION.listingStatus}|self`)?.value).toMatchObject({
			listed: true,
			createdBy: ADA,
			decidedBy: ADMIN
		});

		await t.sharedSpaces.setListed(admin, space, false);
		expect(await t.sharedSpaces.get(space)).toMatchObject({ listed: false });
		expect(
			(await t.audit.list({ subject: space }))
				.map((entry) => entry.action)
				.filter((action) => action.startsWith('listing'))
		).toEqual(['listing.unlist', 'listing.list']);
	});

	it("tells which role someone acts in for a capability, from the root's records", async () => {
		const space = await party('door');

		expect(await t.sharedSpaces.assertCan(await t.viewer(ADA), space, 'people.manage')).toBe('member');
		expect(await t.sharedSpaces.assertCan(await t.viewer(ADMIN), space, 'people.manage')).toBe('admin');
		expect(await t.sharedSpaces.assertCan(await t.viewer(BO), space, 'guests.checkin')).toBe('member');
		await expect(t.sharedSpaces.assertCan(await t.viewer(BO), space, 'events.manage')).rejects.toBeInstanceOf(
			ForbiddenError
		);
	});

	it("refuses spaces that aren't a shared space under the root", async () => {
		const ada = await t.viewer(ADA);

		await expect(t.sharedSpaces.leave(ada, t.spaces.staff)).rejects.toBeInstanceOf(NotFoundError);
		await expect(
			t.sharedSpaces.leave(ada, 'at://did:plc:other/space/computer.localnetwork.space.party/3k')
		).rejects.toBeInstanceOf(NotFoundError);
	});

	it("finishes a member update that failed, on the next reconcile, from the root's records", async () => {
		const space = await party('door');
		vi.spyOn(FakeRootClient.prototype, 'putMember').mockRejectedValueOnce(new Error('space host is down'));

		await t.sharedSpaces.updatePerson(await t.viewer(ADA), space, BO, { role: 'cohost' });
		expect(access(space)[BO]).toEqual({ read: true, write: false });

		// Performers' spaces can't be listed this time, which leaves the parties' to go on.
		vi.spyOn(FakeRootClient.prototype, 'listSpaces').mockRejectedValueOnce(new Error('down'));
		await t.sharedSpaces.reconcile();
		expect(access(space)).toEqual({ [ADA]: readWrite, [BO]: readWrite });
	});

	it('forgets a space that is gone', async () => {
		const space = await party();

		await t.sharedSpaces.forget(space);

		expect(await t.sharedSpaces.get(space)).toBeNull();
		expect(await t.sharedSpaces.people(space, await t.viewer(ADMIN))).toEqual([]);
	});
});
