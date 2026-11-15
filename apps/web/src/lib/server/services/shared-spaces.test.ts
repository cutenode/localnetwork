import type { JsonObject } from '@atproto/lex-json';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION, SPACE_TYPE } from '#lib/nsid.ts';
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
		vi.useRealTimers();
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

	it('lists every space for staff, newest first, with how many people each has and who started it', async () => {
		await t.accounts.recordLogin({
			did: ADA,
			handle: 'ada.test',
			pdsUrl: null,
			grantedScope: 'atproto',
			spacesCapable: true,
			publicWrites: false
		});
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(new Date('2026-10-01T00:00:00Z'));
		const performer = await t.sharedSpaces.create(await t.viewer(ADA), 'performer');
		vi.setSystemTime(new Date('2026-10-02T00:00:00Z'));
		const space = await party();

		expect(await t.sharedSpaces.list()).toEqual([
			{
				uri: space,
				kind: 'party',
				skey: space.split('/').at(-1),
				listed: false,
				people: 2,
				createdBy: { did: ADA, handle: 'ada.test' },
				createdAt: '2026-10-02T00:00:00.000Z'
			},
			expect.objectContaining({ uri: performer, kind: 'performer', people: 1 })
		]);
		// Audit summaries name people by handle when there is one.
		await t.sharedSpaces.removePerson(await t.viewer(ADMIN), performer, ADA);
		expect((await t.audit.list({ action: 'people.remove' }))[0].summary).toBe('Removed @ada.test');
	});

	it("keeps someone's capabilities when an update changes neither role nor capabilities", async () => {
		const space = await party('door');

		await t.sharedSpaces.updatePerson(await t.viewer(ADA), space, BO, {});
		expect(person(space, BO)).toMatchObject({ role: 'door', capabilities: ['guests.checkin'] });
		expect((await t.audit.list({ action: 'people.update' }))[0].changes).toBeNull();
	});

	it('lets staff leave a space nobody else can manage, noting it in their staff role', async () => {
		const space = await t.sharedSpaces.create(await t.viewer(ADMIN), 'party');

		await t.sharedSpaces.leave(await t.viewer(ADMIN), space);
		expect(access(space)).toEqual({});
		expect(await audited('people.leave')).toMatchObject([
			{ actor: ADMIN, actorRole: 'admin', details: { unmanaged: true } }
		]);
	});

	it('starts a listing status when the space has none, created by the staff member listing it', async () => {
		const space = await party();
		const key = `${space}|${ROOT}|${COLLECTION.listingStatus}|self`;
		t.net.spaceRecords.delete(key);

		await t.sharedSpaces.setListed(await t.viewer(ADMIN), space, true);
		expect(t.net.spaceRecords.get(key)?.value).toMatchObject({ listed: true, createdBy: ADMIN, decidedBy: ADMIN });
		expect(await t.sharedSpaces.get(space)).toMatchObject({ listed: true, createdBy: ADMIN });
	});

	it('goes on reconciling the other spaces when one of them cannot be read', async () => {
		const spaces = [await party('door'), await party('door')];
		const down = new Error('space host is down');
		vi.spyOn(FakeRootClient.prototype, 'putMember').mockRejectedValueOnce(down).mockRejectedValueOnce(down);

		for (const space of spaces) await t.sharedSpaces.updatePerson(await t.viewer(ADA), space, BO, { role: 'cohost' });
		const reader = { read: true, write: false };
		expect(spaces.map((space) => access(space)[BO])).toEqual([reader, reader]);

		// The first space reconciled can't be read; the other is still brought in line.
		vi.spyOn(FakeRootClient.prototype, 'listSpaceRecords').mockRejectedValueOnce(down);
		await t.sharedSpaces.reconcile();
		expect(spaces.map((space) => access(space)[BO])).toEqual(expect.arrayContaining([reader, readWrite]));
	});

	it("leaves out a person record in the root's repo that isn't keyed by who it names", async () => {
		const space = await t.sharedSpaces.create(await t.viewer(ADA), 'party');
		const at = new Date().toISOString();
		const record = { subject: BO, role: 'host', capabilities: ['people.manage'], hidden: false, via: 'staff' };
		await t.net.client(ROOT).putSpaceRecord({
			space,
			collection: COLLECTION.person,
			rkey: CY,
			record: { ...record, createdAt: at, updatedAt: at }
		});

		await t.sharedSpaces.syncMembers(space);
		expect(access(space)).toEqual({ [ADA]: readWrite });
		await expect(t.sharedSpaces.assertCan(await t.viewer(BO), space, 'people.manage')).rejects.toBeInstanceOf(
			ForbiddenError
		);
	});

	describe('index', () => {
		const at = '2026-10-01T00:00:00.000Z';

		const rows = async (space: string) => ({
			people: (await t.sharedSpaces.people(space, await t.viewer(ADMIN))).map((view) => view.did),
			invites: await t.db.selectFrom('spaceInvite').select(['did', 'message']).where('spaceUri', '=', space).execute(),
			links: await t.db
				.selectFrom('spaceInviteLink')
				.select(['rkey', 'expiresAt'])
				.where('spaceUri', '=', space)
				.execute(),
			requests: await t.db.selectFrom('spaceJoinRequest').select('did').where('spaceUri', '=', space).execute()
		});

		it("ignores records it can't read, and ones from spaces that aren't shared spaces", async () => {
			const space = await party();
			const before = await rows(space);
			const warn = vi.spyOn(t.core.logger, 'warn');
			const offer = { role: 'door', capabilities: ['guests.checkin'], createdAt: at };

			const records: [string, JsonObject][] = [
				[
					COLLECTION.person,
					{ subject: BO, role: 'door', capabilities: [], hidden: false, via: 'staff', createdAt: at }
				],
				[COLLECTION.invite, { ...offer, subject: BO, invitedBy: ADA }],
				[COLLECTION.inviteLink, { ...offer, createdBy: ADA }],
				[COLLECTION.joinRequest, { subject: BO, createdAt: at }],
				[COLLECTION.listingStatus, { listed: true, createdBy: ADA, createdAt: at }],
				['com.example.other', {}]
			];

			for (const [collection, value] of records) await t.sharedSpaces.index({ space, collection, rkey: CY, value });

			expect(await rows(space)).toEqual(before);
			expect(await t.sharedSpaces.get(space)).toMatchObject({ listed: false });
			expect(warn).toHaveBeenCalledTimes(6);

			for (const other of [t.spaces.staff, 'not a space'])
				await t.sharedSpaces.index({ space: other, collection: COLLECTION.joinRequest, rkey: CY, value: {} });
			expect(warn).toHaveBeenCalledTimes(6);
		});

		it('indexes invites, links and requests to join, and drops each when deleted', async () => {
			const space = await party();
			const offer = { role: 'door', capabilities: ['guests.checkin'], createdAt: at };

			const changes = [
				{ collection: COLLECTION.invite, rkey: CY, value: { ...offer, subject: CY, invitedBy: ADA } },
				{ collection: COLLECTION.inviteLink, rkey: 'link', value: { ...offer, tokenHash: 'hash', createdBy: ADA } },
				{ collection: COLLECTION.joinRequest, rkey: CY, value: { subject: CY, createdAt: at } }
			];

			for (const change of changes) await t.sharedSpaces.index({ space, ...change });
			expect(await rows(space)).toEqual({
				people: [ADA, BO],
				invites: [{ did: CY, message: null }],
				links: [{ rkey: 'link', expiresAt: null }],
				requests: [{ did: CY }]
			});

			for (const { collection, rkey } of [...changes, { collection: 'com.example.other', rkey: CY }])
				await t.sharedSpaces.unindex({ space, collection, rkey });
			await t.sharedSpaces.unindex({ space, collection: COLLECTION.listingStatus, rkey: 'other' });
			expect(await rows(space)).toEqual({ people: [ADA, BO], invites: [], links: [], requests: [] });
			expect(await t.sharedSpaces.get(space)).not.toBeNull();

			await t.sharedSpaces.unindex({ space, collection: COLLECTION.listingStatus, rkey: 'self' });
			expect(await t.sharedSpaces.get(space)).toBeNull();
		});
	});

	describe('acts', () => {
		const basics = { name: 'Crew' };

		const start = async (slug = 'crew') => t.sharedSpaces.createAct(await t.viewer(ADA), { slug, form: 'duo', basics });

		const page = (space: string) => t.net.spaceRecords.get(`${space}|${ROOT}|${COLLECTION.performer}|self`);

		it('starts an act with its page, and its creator in it as a member', async () => {
			const space = await start();

			expect(page(space)?.value).toEqual({
				$type: COLLECTION.performer,
				subject: { $type: `${COLLECTION.performer}#act`, slug: 'crew', form: 'duo', basics, performer: {} },
				createdAt: expect.any(String),
				updatedAt: expect.any(String)
			});
			expect(person(space, ADA)).toMatchObject({ role: 'member', via: 'created' });
			expect(access(space)).toEqual({ [ADA]: readWrite });
			expect(await t.performerRegistry.bySlug('crew')).toMatchObject({
				kind: 'act',
				space,
				act: { form: 'duo', basics }
			});
			expect(await audited('space.create')).toEqual([
				{ actor: ADA, actorRole: 'member', subject: space, details: { person: ADA, role: 'member' } }
			]);
		});

		it('refuses an address another act has, even when two ask at once, and one that is not an address', async () => {
			await start();

			await expect(start()).rejects.toThrow(new UserError('Another act has that address'));
			await expect(start('Crew Two')).rejects.toBeInstanceOf(UserError);

			const both = await Promise.allSettled([start('twins'), start('twins')]);
			expect(both.map((result) => result.status).sort()).toEqual(['fulfilled', 'rejected']);
			expect(await t.db.selectFrom('performer').select('slug').orderBy('slug').execute()).toEqual([
				{ slug: 'crew' },
				{ slug: 'twins' }
			]);
			// The one refused never started a space.
			expect([...t.net.spaces.keys()].filter((uri) => uri.includes(SPACE_TYPE.performer))).toHaveLength(2);
		});

		it('saves the page for people with page.edit and staff, over the version its form was opened on', async () => {
			const space = await start();
			const admin = await t.viewer(ADMIN);
			await t.sharedSpaces.addPerson(admin, space, { did: BO, role: 'manager' });

			const input = {
				form: 'group' as const,
				basics: { name: 'Crew Three' },
				performer: { genres: ['techno'] },
				avatar: { upload: { bytes: new Uint8Array([1, 2, 3]), mimeType: 'image/png' } },
				swap: page(space)!.cid
			};

			await expect(t.sharedSpaces.updatePage(await t.viewer(CY), space, input)).rejects.toBeInstanceOf(ForbiddenError);
			await t.sharedSpaces.updatePage(await t.viewer(BO), space, input);

			const saved = page(space)!;
			expect(saved.value.subject).toEqual({
				$type: `${COLLECTION.performer}#act`,
				slug: 'crew',
				form: 'group',
				basics: { name: 'Crew Three', avatar: expect.objectContaining({ $type: 'blob', mimeType: 'image/png' }) },
				performer: { genres: ['techno'] }
			});
			expect(await t.performerRegistry.bySlug('crew')).toMatchObject({ cid: saved.cid, act: { form: 'group' } });
			expect(await audited('page.update')).toEqual([
				{ actor: BO, actorRole: 'member', subject: space, details: { slug: 'crew' } }
			]);

			// The form was opened on the version before.
			await expect(t.sharedSpaces.updatePage(await t.viewer(BO), space, input)).rejects.toThrow(
				new UserError('The page changed since you opened it. Reload to see the latest, then save again.')
			);

			await t.sharedSpaces.updatePage(admin, space, { ...input, avatar: 'keep', swap: saved.cid });
			// The same page, avatar kept.
			expect(page(space)?.value.subject).toEqual(saved.value.subject);
		});

		it('refuses to save the page for someone whose page.edit was taken away, and for a space with no act', async () => {
			const space = await start();
			await t.sharedSpaces.addPerson(await t.viewer(ADMIN), space, { did: BO, role: 'manager' });
			await t.sharedSpaces.updatePerson(await t.viewer(ADA), space, BO, { capabilities: ['bookings.answer'] });
			const input = { form: 'duo' as const, basics, performer: {}, avatar: 'remove' as const, swap: page(space)!.cid };

			await expect(t.sharedSpaces.updatePage(await t.viewer(BO), space, input)).rejects.toBeInstanceOf(ForbiddenError);
			await expect(t.sharedSpaces.updatePage(await t.viewer(ADA), await party(), input)).rejects.toBeInstanceOf(
				NotFoundError
			);
		});
	});

	it('forgets a space that is gone', async () => {
		const space = await party();

		await t.sharedSpaces.forget(space);

		expect(await t.sharedSpaces.get(space)).toBeNull();
		expect(await t.sharedSpaces.people(space, await t.viewer(ADMIN))).toEqual([]);
	});
});
