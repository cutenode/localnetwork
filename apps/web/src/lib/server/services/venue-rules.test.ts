import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { ForbiddenError, NotFoundError, UserError } from '../core';
import { createTestServices, ROOT } from '../testing/core';
import type { VenueView } from './venues';

const ADMIN = 'did:plc:admin';

const OWNER = 'did:plc:owner';

const BOOKER = 'did:plc:booker';

const PARTY = `at://${ROOT}/${COLLECTION.listing}/3party`;

const DJ = `at://did:plc:dj/${COLLECTION.profile}/self`;

describe('venue rules and settings', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let venue: VenueView;
	let owner: Viewer;
	let booker: Viewer;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN] });
		const admin = await t.viewer(ADMIN);
		venue = await t.venues.add(admin, { name: 'Tresor', address: { country: 'DE', locality: 'Berlin' } });
		await t.sharedSpaces.addPerson(admin, venue.space, { did: OWNER, role: 'owner' });
		await t.sharedSpaces.addPerson(admin, venue.space, { did: BOOKER, role: 'booker' });
		owner = await t.viewer(OWNER);
		booker = await t.viewer(BOOKER);
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const audited = async (action: string) => {
		await t.audit.idle();

		return t.db.selectFrom('auditEntry').select(['actorDid', 'summary']).where('action', '=', action).execute();
	};

	describe('events', () => {
		it('lists an event at once under Anyone can list, waiting for the venue until confirmed', async () => {
			expect(await t.venueRules.event(venue.space, [PARTY, DJ], false)).toEqual({ listed: true, confirmed: false });
			expect(await t.venueRules.event(venue.space, [PARTY, DJ], true)).toEqual({ listed: true, confirmed: true });
		});

		it('lists an event only once confirmed under Venue approves', async () => {
			await t.venues.setApproval(owner, venue.space, 'approval');

			expect(await t.venueRules.event(venue.space, [PARTY], false)).toEqual({ listed: false, confirmed: false });
			expect(await t.venueRules.event(venue.space, [PARTY], true)).toEqual({ listed: true, confirmed: true });
		});

		it("never confirms a blocked party's or performer's event, which looks like any waiting for the venue", async () => {
			await t.venues.block(owner, venue.space, DJ);
			const waiting = await t.venueRules.event(venue.space, [PARTY], false);

			expect(await t.venueRules.event(venue.space, [PARTY, DJ], true)).toEqual(waiting);
			expect(await t.venueRules.event(venue.space, [DJ], true)).toEqual(waiting);

			await t.venues.setApproval(owner, venue.space, 'approval');
			expect(await t.venueRules.event(venue.space, [PARTY, DJ], true)).toEqual(
				await t.venueRules.event(venue.space, [PARTY], false)
			);
		});

		it('lists nothing at a venue that is not there', async () => {
			expect(await t.venueRules.event(`${venue.space}x`, [], true)).toEqual({ listed: false, confirmed: false });
		});
	});

	it("leaves a blocked profile, act or party's relationship or inquiry without the venue", async () => {
		expect(await t.venueRules.shows(venue.space, [PARTY])).toBe(true);
		expect(await t.venueRules.shows(venue.space, [])).toBe(true);

		await t.venues.block(owner, venue.space, PARTY);

		expect(await t.venueRules.shows(venue.space, [PARTY])).toBe(false);
		expect(await t.venueRules.shows(venue.space, [DJ, PARTY])).toBe(false);
		expect(await t.venueRules.shows(venue.space, [DJ])).toBe(true);
	});

	it('sets the approval mode for people with page.edit and staff, and audits it', async () => {
		await expect(t.venues.setApproval(booker, venue.space, 'approval')).rejects.toBeInstanceOf(ForbiddenError);

		await t.venues.setApproval(owner, venue.space, 'approval');
		expect(await t.venues.bySpace(venue.space)).toMatchObject({ approval: 'approval' });
		const stored = t.net.spaceRecords.get(`${venue.space}|${ROOT}|${COLLECTION.venue}|self`)!.value;
		expect(() => validateRecord(COLLECTION.venue, stored)).not.toThrow();

		await t.venues.setApproval(await t.viewer(ADMIN), venue.space, 'open');
		expect(await t.venues.bySpace(venue.space)).toMatchObject({ approval: 'open' });
		expect(await audited('page.update')).toEqual([
			{ actorDid: OWNER, summary: 'Set Tresor to Venue approves' },
			{ actorDid: ADMIN, summary: 'Set Tresor to Anyone can list' }
		]);
	});

	it("blocks and unblocks for people with venue.block, as a public record in the root's repo", async () => {
		await expect(t.venues.block(booker, venue.space, PARTY)).rejects.toBeInstanceOf(ForbiddenError);

		await t.venues.block(owner, venue.space, PARTY);
		const [block] = await t.venues.blocks(venue.space);
		expect(block).toMatchObject({ subject: PARTY });
		const stored = t.net.records.get(`${ROOT}|${COLLECTION.venueBlock}|${block.rkey}`)!.value;
		expect(stored).toMatchObject({ venue: venue.space, subject: PARTY });
		expect(() => validateRecord(COLLECTION.venueBlock, stored)).not.toThrow();
		// Nothing in the venue's space: the block is public, like a Bluesky block.
		expect([...t.net.spaceRecords.keys()].some((key) => key.includes(COLLECTION.venueBlock))).toBe(false);

		await expect(t.venues.block(owner, venue.space, PARTY)).rejects.toThrow(new UserError("They're already blocked"));
		await expect(t.venues.unblock(booker, venue.space, block.rkey)).rejects.toBeInstanceOf(ForbiddenError);

		await t.venues.unblock(owner, venue.space, block.rkey);
		expect(await t.venues.blocks(venue.space)).toEqual([]);
		expect(t.net.records.has(`${ROOT}|${COLLECTION.venueBlock}|${block.rkey}`)).toBe(false);
		await expect(t.venues.unblock(owner, venue.space, block.rkey)).rejects.toBeInstanceOf(NotFoundError);
		expect(await audited('venue.block')).toEqual([{ actorDid: OWNER, summary: 'Blocked someone from the venue' }]);
		expect(await audited('venue.unblock')).toEqual([{ actorDid: OWNER, summary: 'Took a block away' }]);
	});

	it('refuses to set the approval mode of a venue space with no venue in it', async () => {
		const empty = await t.sharedSpaces.create(owner, 'venue');

		await expect(t.venues.setApproval(owner, empty, 'approval')).rejects.toBeInstanceOf(NotFoundError);
	});

	it("won't take away another venue's block", async () => {
		const other = await t.venues.add(await t.viewer(ADMIN), {
			name: 'Berghain',
			address: { country: 'DE', locality: 'Berlin' }
		});

		await t.venues.block(await t.viewer(ADMIN), other.space, PARTY);
		const [block] = await t.venues.blocks(other.space);

		await expect(t.venues.unblock(owner, venue.space, block.rkey)).rejects.toBeInstanceOf(NotFoundError);
		expect(await t.venues.blocks(other.space)).toHaveLength(1);
	});

	it('keeps its blocks when the venue space is forgotten, since a block outlives a merge', async () => {
		await t.venues.block(owner, venue.space, PARTY);

		await t.sharedSpaces.forget(venue.space);

		expect(await t.venues.blocks(venue.space)).toMatchObject([{ subject: PARTY }]);
	});

	it("indexes blocks from the root's own repo only, valid by the lexicon, and drops ones that are gone", async () => {
		await t.venues.block(owner, venue.space, PARTY);
		const [kept] = await t.venues.blocks(venue.space);
		const root = await t.core.root();
		const record = { venue: venue.space, subject: DJ, createdAt: '2020-01-01T00:00:00.000Z' };

		await root.putRecord({ collection: COLLECTION.venueBlock, rkey: '3kdj', record });
		await root.putRecord({
			collection: COLLECTION.venueBlock,
			rkey: '3kbad',
			record: { venue: 'not a uri', subject: DJ }
		});
		// Someone else's block record names the venue too, but only the root's count.
		await t.net.client('did:plc:dj').putRecord({ collection: COLLECTION.venueBlock, rkey: '3kother', record });
		await t.db.deleteFrom('venueBlock').execute();

		await t.venues.reconcileBlocks();
		expect((await t.venues.blocks(venue.space)).map(({ rkey }) => rkey)).toEqual(['3kdj', kept.rkey]);

		await root.deleteRecord({ collection: COLLECTION.venueBlock, rkey: '3kdj' });
		await root.deleteRecord({ collection: COLLECTION.venueBlock, rkey: kept.rkey });
		await t.venues.reconcileBlocks();
		expect(await t.venues.blocks(venue.space)).toEqual([]);
	});
});
