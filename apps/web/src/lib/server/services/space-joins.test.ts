import { XRPCError } from '@atproto/api';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { ForbiddenError, NotFoundError, UserError } from '../core';
import { RateLimiter } from '../rate-limit';
import { createTestServices, ROOT } from '../testing/core';
import { SpaceJoins } from './space-joins';

const ADA = 'did:plc:ada';

const BO = 'did:plc:bo';

const CY = 'did:plc:cy';

describe('SpaceJoins', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let space: string;

	beforeEach(async () => {
		t = await createTestServices();
		space = await t.sharedSpaces.create(await t.viewer(ADA), 'party');
	});
	afterEach(async () => {
		vi.useRealTimers();
		await t.audit.idle();
		await t.db.destroy();
	});

	const members = () => [...t.net.spaces.get(space)!.members.keys()].sort();
	const people = async () => (await t.sharedSpaces.people(space, await t.viewer(ADA))).map((view) => view.did);
	const stored = (collection: string, rkey: string) => t.net.spaceRecords.get(`${space}|${ROOT}|${collection}|${rkey}`);
	const actions = async () => (await t.audit.list({ subject: space })).map((entry) => entry.action).toReversed();

	describe('invites', () => {
		it("don't make anyone a member until they accept, hidden if they choose", async () => {
			await t.spaceJoins.invite(await t.viewer(ADA), space, { did: BO, role: 'cohost', message: 'Join us' });

			expect(members()).toEqual([ADA]);
			expect(await t.spaceJoins.invitesFor(BO)).toMatchObject([
				{ space, kind: 'party', role: 'cohost', invitedBy: ADA, message: 'Join us' }
			]);

			await t.spaceJoins.accept(await t.viewer(BO), space, { hidden: true });

			expect(members()).toEqual([ADA, BO]);
			expect(stored(COLLECTION.invite, BO)).toBeUndefined();
			expect(stored(COLLECTION.person, BO)?.value).toMatchObject({
				role: 'cohost',
				hidden: true,
				via: 'invite',
				addedBy: ADA,
				source: `${space}/${ROOT}/${COLLECTION.invite}/${BO}`
			});
			expect(await t.spaceJoins.invitesFor(BO)).toEqual([]);
			expect(await t.sharedSpaces.people(space, null)).toMatchObject([{ did: ADA }]);
			expect(await actions()).toEqual(['space.create', 'people.invite', 'people.join']);
		});

		it('can be declined or taken back, and need people.manage to send', async () => {
			const ada = await t.viewer(ADA);

			await t.spaceJoins.invite(ada, space, { did: BO, role: 'door' });
			await expect(t.spaceJoins.invite(await t.viewer(BO), space, { did: CY, role: 'door' })).rejects.toBeInstanceOf(
				ForbiddenError
			);
			await expect(t.spaceJoins.invite(ada, space, { did: BO, role: 'door' })).rejects.toThrow('already invited');
			await expect(t.spaceJoins.invite(ada, space, { did: ADA, role: 'door' })).rejects.toThrow('already in it');
			await expect(t.spaceJoins.invite(ada, space, { did: CY, role: 'owner' })).rejects.toThrow('no owner role');

			await t.spaceJoins.decline(await t.viewer(BO), space);
			expect(stored(COLLECTION.invite, BO)).toBeUndefined();
			await expect(t.spaceJoins.accept(await t.viewer(BO), space, { hidden: false })).rejects.toBeInstanceOf(
				NotFoundError
			);
			await expect(t.spaceJoins.decline(await t.viewer(BO), space)).rejects.toBeInstanceOf(NotFoundError);

			await t.spaceJoins.invite(ada, space, { did: CY, role: 'door' });
			await t.spaceJoins.revokeInvite(ada, space, CY);
			expect(await t.spaceJoins.invitesFor(CY)).toEqual([]);
			await expect(t.spaceJoins.revokeInvite(ada, space, CY)).rejects.toBeInstanceOf(NotFoundError);
			expect(members()).toEqual([ADA]);
		});

		it.each([1, 2])("are accepted all at once or not at all, when the root's write %i fails", async (failing) => {
			await t.spaceJoins.invite(await t.viewer(ADA), space, { did: BO, role: 'door' });
			// The audit log writes from the root too.
			await t.audit.idle();
			const maybeFail = t.net.maybeFail.bind(t.net);
			let writes = 0;

			vi.spyOn(t.net, 'maybeFail').mockImplementation((did) => {
				if (did === ROOT && (writes += 1) === failing) throw new XRPCError(502, 'UpstreamFailure');
				maybeFail(did);
			});

			await t.spaceJoins.accept(await t.viewer(BO), space, { hidden: false }).catch(() => undefined);
			vi.restoreAllMocks();
			// Either the invite is still there and they're not in, or they're in and it's gone.
			expect(stored(COLLECTION.invite, BO) === undefined).toBe(stored(COLLECTION.person, BO) !== undefined);

			if (stored(COLLECTION.invite, BO)) await t.spaceJoins.accept(await t.viewer(BO), space, { hidden: false });
			else await t.sharedSpaces.syncMembers(space);
			expect(members()).toEqual([ADA, BO]);
		});

		it('run out once they expire', async () => {
			vi.useFakeTimers({ toFake: ['Date'] });
			await t.spaceJoins.invite(await t.viewer(ADA), space, { did: BO, role: 'door', expiresInDays: 1 });
			vi.setSystemTime(Date.now() + 2 * 24 * 60 * 60 * 1000);

			expect(await t.spaceJoins.invitesFor(BO)).toEqual([]);
			await expect(t.spaceJoins.accept(await t.viewer(BO), space, { hidden: false })).rejects.toBeInstanceOf(
				NotFoundError
			);
		});
	});

	describe('invite links', () => {
		it('keep only the token hash, and never let anyone manage people', async () => {
			const { token, rkey } = await t.spaceJoins.createLink(await t.viewer(ADA), space, { role: 'host' });

			expect(stored(COLLECTION.inviteLink, rkey)?.value.capabilities).toEqual([
				'page.edit',
				'listing.request',
				'events.manage',
				'bookings.send',
				'guests.manage',
				'guests.checkin'
			]);

			const everything = JSON.stringify([...t.net.spaceRecords.values()]);
			expect(everything).not.toContain(token);
			expect(JSON.stringify(await t.db.selectFrom('spaceInviteLink').selectAll().execute())).not.toContain(token);
			expect(JSON.stringify(await t.audit.list())).not.toContain(token);

			expect(await t.spaceJoins.joinByLink(await t.viewer(BO), token, { hidden: false })).toBe(space);
			expect(stored(COLLECTION.person, BO)?.value).toMatchObject({ role: 'host', via: 'link', addedBy: ADA });
			expect(stored(COLLECTION.person, BO)?.value.capabilities).not.toContain('people.manage');
			expect(members()).toEqual([ADA, BO]);

			// The link stays for the next person, but not for someone already in.
			await expect(t.spaceJoins.joinByLink(await t.viewer(BO), token, { hidden: false })).rejects.toThrow(
				'already in it'
			);
			await t.spaceJoins.joinByLink(await t.viewer(CY), token, { hidden: true });
			expect(await people()).toEqual([ADA, BO, CY]);
		});

		it('stop working once turned off or expired, and refuse made-up tokens', async () => {
			vi.useFakeTimers({ toFake: ['Date'] });
			const ada = await t.viewer(ADA);
			const revoked = await t.spaceJoins.createLink(ada, space, { role: 'door' });
			const expiring = await t.spaceJoins.createLink(ada, space, { role: 'door', expiresInDays: 1 });

			await expect(t.spaceJoins.revokeLink(await t.viewer(BO), space, revoked.rkey)).rejects.toBeInstanceOf(
				ForbiddenError
			);
			await t.spaceJoins.revokeLink(ada, space, revoked.rkey);
			await expect(t.spaceJoins.revokeLink(ada, space, revoked.rkey)).rejects.toBeInstanceOf(NotFoundError);
			vi.setSystemTime(Date.now() + 2 * 24 * 60 * 60 * 1000);

			const bo = await t.viewer(BO);

			for (const token of [revoked.token, expiring.token, 'nope', 'x'.repeat(43)])
				await expect(t.spaceJoins.joinByLink(bo, token, { hidden: false })).rejects.toThrow('That link is gone');
			expect(members()).toEqual([ADA]);
		});

		it("are checked against the root's record, not an index that's behind", async () => {
			const { token, rkey } = await t.spaceJoins.createLink(await t.viewer(ADA), space, { role: 'door' });
			t.net.spaceRecords.delete(`${space}|${ROOT}|${COLLECTION.inviteLink}|${rkey}`);

			await expect(t.spaceJoins.joinByLink(await t.viewer(BO), token, { hidden: false })).rejects.toBeInstanceOf(
				NotFoundError
			);
		});
	});

	describe('requests to join', () => {
		it('wait for someone with people.manage, who picks the role', async () => {
			await t.spaceJoins.requestToJoin(await t.viewer(BO), space, { message: 'Can I help?', hidden: true });

			expect(members()).toEqual([ADA]);
			expect(await t.spaceJoins.requestsFor(space)).toMatchObject([{ did: BO, message: 'Can I help?' }]);
			await expect(t.spaceJoins.approve(await t.viewer(CY), space, BO, 'door')).rejects.toBeInstanceOf(ForbiddenError);

			await t.spaceJoins.approve(await t.viewer(ADA), space, BO, 'door');

			expect(members()).toEqual([ADA, BO]);
			expect(stored(COLLECTION.person, BO)?.value).toMatchObject({ role: 'door', hidden: true, via: 'request' });
			expect(await t.spaceJoins.requestsFor(space)).toEqual([]);
			await expect(t.spaceJoins.approve(await t.viewer(ADA), space, BO, 'door')).rejects.toBeInstanceOf(NotFoundError);
		});

		it('can be turned down, and are refused from someone in, invited or who already asked', async () => {
			const ada = await t.viewer(ADA);
			const bo = await t.viewer(BO);

			await t.spaceJoins.requestToJoin(bo, space, { hidden: false });
			await expect(t.spaceJoins.requestToJoin(bo, space, { hidden: false })).rejects.toThrow('already asked');
			await expect(t.spaceJoins.requestToJoin(ada, space, { hidden: false })).rejects.toThrow('already in it');
			await expect(t.spaceJoins.reject(bo, space, BO)).rejects.toBeInstanceOf(ForbiddenError);

			await t.spaceJoins.reject(ada, space, BO);
			expect(stored(COLLECTION.joinRequest, BO)).toBeUndefined();
			await expect(t.spaceJoins.reject(ada, space, BO)).rejects.toBeInstanceOf(NotFoundError);

			await t.spaceJoins.invite(ada, space, { did: CY, role: 'door' });
			await expect(t.spaceJoins.requestToJoin(await t.viewer(CY), space, { hidden: false })).rejects.toThrow(
				'accept the invite instead'
			);
			expect(await actions()).toEqual(['space.create', 'people.request', 'people.reject', 'people.invite']);
		});

		it('are answered by an invite accepted later, and answer the invite when approved', async () => {
			const ada = await t.viewer(ADA);

			await t.spaceJoins.requestToJoin(await t.viewer(BO), space, { hidden: false });
			await t.spaceJoins.invite(ada, space, { did: BO, role: 'door' });
			await t.spaceJoins.accept(await t.viewer(BO), space, { hidden: false });
			expect([stored(COLLECTION.joinRequest, BO), stored(COLLECTION.invite, BO)]).toEqual([undefined, undefined]);

			// Staff can invite someone who then asks too: approving the request answers both.
			await t.spaceJoins.requestToJoin(await t.viewer(CY), space, { hidden: false });
			await t.spaceJoins.invite(ada, space, { did: CY, role: 'door' });
			await t.spaceJoins.approve(ada, space, CY, 'cohost');
			expect([stored(COLLECTION.joinRequest, CY), stored(COLLECTION.invite, CY)]).toEqual([undefined, undefined]);
			expect(await t.spaceJoins.requestsFor(space)).toEqual([]);
		});
	});

	it('limits how often one account joins, invites or asks', async () => {
		const joins = new SpaceJoins(t.core, t.sharedSpaces, new RateLimiter(1, 0));
		const bo = await t.viewer(BO);

		await joins.requestToJoin(bo, space, { hidden: false });
		const limited = joins.joinByLink(bo, 'x'.repeat(43), { hidden: false });

		await expect(limited).rejects.toBeInstanceOf(UserError);
		await expect(limited).rejects.toMatchObject({ status: 429 });
	});
});
