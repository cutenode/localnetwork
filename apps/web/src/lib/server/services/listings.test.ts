import * as fc from 'fast-check';
import type { JsonObject } from '@atproto/lex-json';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { validateRecord } from '../atproto/lexicons';
import { parseSpaceUri, sharedSpace } from '../atproto/spaces';
import { FakeRootClient } from '../testing/fakes';
import { createTestServices, ROOT } from '../testing/core';
import type { ListingFields } from './listings';

const ADMIN = 'did:plc:admin';

const ADA = 'did:plc:ada';

const BO = 'did:plc:bo';

const CY = 'did:plc:cy';

describe('ListingWriter', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let fields: ListingFields | null;
	let space: string;

	beforeEach(async () => {
		fields = { name: 'Night Shift', slug: 'night-shift', fields: { city: 'Berlin' } };
		t = await createTestServices({
			admins: [ADMIN],
			listingSources: { party: { publicFields: async () => fields }, performer: { publicFields: async () => fields } }
		});
		space = await t.sharedSpaces.create(await t.viewer(ADA), 'party');
	});
	afterEach(async () => {
		vi.restoreAllMocks();
		vi.useRealTimers();
		await t.audit.idle();
		await t.db.destroy();
	});

	const listing = (inSpace = space) =>
		t.net.records.get(`${ROOT}|${COLLECTION.listing}|${parseSpaceUri(inSpace)!.skey}`)?.value;

	const list = async () => t.sharedSpaces.setListed(await t.viewer(ADMIN), space, true);

	it("writes nothing for a space staff haven't listed", async () => {
		await t.sharedSpaces.addPerson(await t.viewer(ADMIN), space, { did: BO, role: 'cohost' });

		expect(listing()).toBeUndefined();
	});

	it('lists only the people in a listed role who have not hidden, never what else the space holds', async () => {
		const admin = await t.viewer(ADMIN);
		await t.sharedSpaces.addPerson(admin, space, { did: BO, role: 'door' });
		await t.sharedSpaces.addPerson(admin, space, { did: CY, role: 'cohost', hidden: true });
		await t.spaceJoins.invite(await t.viewer(ADA), space, { did: 'did:plc:di', role: 'host' });
		await list();

		const record = listing()!;
		expect(record).toMatchObject({
			$type: COLLECTION.listing,
			kind: 'party',
			name: 'Night Shift',
			slug: 'night-shift',
			fields: { city: 'Berlin' },
			people: [{ did: ADA, role: 'host' }]
		});
		expect(() => validateRecord(COLLECTION.listing, record)).not.toThrow();
		expect(JSON.stringify(record)).not.toMatch(/capabilit|via|did:plc:di|did:plc:bo|did:plc:cy/);

		// Cy stops hiding and the listing follows.
		await t.sharedSpaces.setHidden(await t.viewer(CY), space, CY, false);
		expect(listing()?.people).toEqual([
			{ did: ADA, role: 'host' },
			{ did: CY, role: 'cohost' }
		]);
	});

	it('lists a performer as an act, and takes the listing down once unlisted or with nothing to show', async () => {
		const act = await t.sharedSpaces.create(await t.viewer(ADA), 'performer');
		await t.sharedSpaces.setListed(await t.viewer(ADMIN), act, true);
		expect(listing(act)).toMatchObject({ kind: 'act', people: [{ did: ADA, role: 'member' }] });

		await t.sharedSpaces.setListed(await t.viewer(ADMIN), act, false);
		expect(listing(act)).toBeUndefined();

		await list();
		fields = null;
		await t.listings.refresh(space);
		expect(listing()).toBeUndefined();
	});

	it("doesn't write a listing that hasn't changed", async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		await list();
		const first = listing()?.updatedAt;
		vi.setSystemTime(Date.now() + 60_000);

		await t.sharedSpaces.setHidden(await t.viewer(ADA), space, ADA, false);
		await t.listings.refresh(space);
		await t.listings.reconcile();
		expect(listing()?.updatedAt).toBe(first);

		fields = { name: 'Night Shift 2' };
		await t.listings.refresh(space);
		expect(listing()).toMatchObject({ name: 'Night Shift 2' });
		expect(listing()?.updatedAt).not.toBe(first);
	});

	it('never writes a listing that names someone hidden, keeping the last one', async () => {
		await t.sharedSpaces.addPerson(await t.viewer(ADMIN), space, { did: BO, role: 'cohost', hidden: true });
		await list();
		const before = listing();

		fields = { name: 'Night Shift', fields: { credits: [BO] } };
		await t.listings.refresh(space);

		expect(listing()).toEqual(before);
	});

	it('writes over the listing it read, reading it again once if it changed in between', async () => {
		await list();
		fields = { name: 'Renamed' };
		vi.spyOn(FakeRootClient.prototype, 'getRecord').mockResolvedValueOnce(null);

		await t.listings.refresh(space);
		expect(listing()).toMatchObject({ name: 'Renamed' });

		fields = { name: 'Renamed again' };
		vi.spyOn(FakeRootClient.prototype, 'getRecord').mockResolvedValue(null);
		await t.listings.refresh(space);
		expect(listing()).toMatchObject({ name: 'Renamed' });
	});

	it('puts back a listing that drifted, and deletes listings of spaces that are gone', async () => {
		await list();
		const root = t.net.client(ROOT);
		const rkey = parseSpaceUri(space)!.skey;
		await root.putRecord({ collection: COLLECTION.listing, rkey, record: { ...listing()!, name: 'Edited' } });
		await root.putRecord({ collection: COLLECTION.listing, rkey: '3gone', record: listing()! });
		// A space the index hasn't caught up with yet keeps its listing.
		const unindexed = await t.sharedSpaces.create(await t.viewer(ADA), 'venue');
		await t.db.deleteFrom('sharedSpace').where('spaceUri', '=', unindexed).execute();
		await root.putRecord({ collection: COLLECTION.listing, rkey: parseSpaceUri(unindexed)!.skey, record: listing()! });

		await t.listings.reconcile();

		expect(listing()).toMatchObject({ name: 'Night Shift' });
		expect(t.net.records.has(`${ROOT}|${COLLECTION.listing}|3gone`)).toBe(false);
		expect(listing(unindexed)).toBeDefined();
	});

	it('takes the listing down with a space that is gone', async () => {
		await list();

		await t.sharedSpaces.forget(space);

		expect(listing()).toBeUndefined();
	});

	it("follows the space's people as they sync", async () => {
		await list();
		const now = new Date().toISOString();

		const person = {
			subject: BO,
			role: 'host',
			capabilities: [],
			hidden: false,
			via: 'staff',
			createdAt: now,
			updatedAt: now
		};

		await t.sharedSpaces.index({ space, collection: COLLECTION.person, rkey: BO, value: person });
		await t.listings.refresh(space);

		expect(listing()?.people).toEqual([
			{ did: ADA, role: 'host' },
			{ did: BO, role: 'host' }
		]);
	});

	it('never lets a hidden person into a listing, whoever is in the space', async () => {
		const roles = ['host', 'cohost', 'door'];
		const dids = ['did:plc:p1', 'did:plc:p2', 'did:plc:p3', 'did:plc:p4'];
		const personArb = fc.record({ role: fc.constantFrom(...roles), hidden: fc.boolean() });
		let n = 0;

		await fc.assert(
			fc.asyncProperty(fc.dictionary(fc.constantFrom(...dids), personArb), fc.boolean(), async (people, credits) => {
				const party = sharedSpace(ROOT, 'party', `3prop${(n += 1)}`);
				const now = new Date().toISOString();
				const status = { listed: true, createdBy: ADA, createdAt: now };
				fields = { name: 'Prop', ...(credits && { fields: { credits: dids } }) };

				await t.sharedSpaces.index({ space: party, collection: COLLECTION.listingStatus, rkey: 'self', value: status });

				for (const [did, { role, hidden }] of Object.entries(people)) {
					const value: JsonObject = {
						subject: did,
						role,
						capabilities: [],
						hidden,
						via: 'staff',
						createdAt: now,
						updatedAt: now
					};

					await t.sharedSpaces.index({ space: party, collection: COLLECTION.person, rkey: did, value });
				}

				await t.listings.refresh(party);
				const written = JSON.stringify(listing(party) ?? {});
				const hidden = Object.entries(people).filter(([, person]) => person.hidden);

				expect(hidden.filter(([did]) => written.includes(did))).toEqual([]);

				// Credits naming someone hidden keep the listing from being written at all.
				const listed = Object.entries(people)
					.filter(([, person]) => !person.hidden && person.role !== 'door')
					.toSorted(([a], [b]) => a.localeCompare(b))
					.map(([did, { role }]) => ({ did, role }));

				expect(listing(party)?.people).toEqual(credits && hidden.length > 0 ? undefined : listed);
			})
		);
	});
});
