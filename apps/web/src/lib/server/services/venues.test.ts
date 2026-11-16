import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { VenueRecord, Viewer } from '#lib/types.ts';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import { parseSpaceUri } from '../atproto/spaces';
import { createTestServices, ROOT } from '../testing/core';
import type { VenueInput } from './venues';

const ADMIN = 'did:plc:admin';

const ADA = 'did:plc:ada';

const BERLIN: VenueRecord['address'] = { country: 'DE', locality: 'Berlin' };

const TRESOR: VenueInput = {
	name: 'Tresor',
	address: { ...BERLIN, street: 'Köpenicker Str. 70' },
	geo: { latitude: '52.510', longitude: '13.420' },
	fsq: { fsq_place_id: '4adcdaeef964a520f35b21e3' }
};

describe('VenueService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let ada: Viewer;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], flagOverrides: { spacePages: true } });
		ada = await t.viewer(ADA);
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const listing = (space: string) =>
		t.net.records.get(`${ROOT}|${COLLECTION.listing}|${parseSpaceUri(space)!.skey}`)?.value;

	it('adds a venue the root writes into a venue space with nobody in it, open by default', async () => {
		const venue = await t.venues.add(ada, { ...TRESOR, name: '  Tresor ' });

		expect(venue).toMatchObject({
			skey: parseSpaceUri(venue.space)!.skey,
			name: 'Tresor',
			address: TRESOR.address,
			geo: TRESOR.geo,
			fsqPlaceId: TRESOR.fsq!.fsq_place_id,
			approval: 'open'
		});
		expect(parseSpaceUri(venue.space)).toMatchObject({ authority: ROOT, type: 'computer.localnetwork.space.venue' });

		const stored = t.net.spaceRecords.get(`${venue.space}|${ROOT}|${COLLECTION.venue}|self`);
		expect(() => validateRecord(COLLECTION.venue, stored!.value)).not.toThrow();
		// Adding it doesn't make Ada its owner: that takes a confirmed claim.
		expect(await t.db.selectFrom('spacePerson').selectAll().where('spaceUri', '=', venue.space).execute()).toEqual([]);
		expect(t.net.spaces.get(venue.space)?.members.has(ADA)).toBeFalsy();
		expect(await t.venues.bySkey(venue.skey)).toEqual(venue);
		expect(await t.venues.byPlaceId(TRESOR.fsq!.fsq_place_id)).toEqual(venue);
		expect(await t.venues.bySkey('3berghain')).toBeNull();

		await t.audit.idle();
		const entry = await t.db.selectFrom('auditEntry').selectAll().where('subject', '=', venue.space).executeTakeFirst();
		expect(entry).toMatchObject({ actorDid: ADA, action: 'space.create', summary: 'Added Tresor' });
	});

	it('returns the venue that already has the place instead of adding it again', async () => {
		const first = await t.venues.add(ada, TRESOR);
		const again = await t.venues.add(await t.viewer(ADMIN), { ...TRESOR, name: 'Tresor Berlin' });

		expect(again).toEqual(first);
		expect(await t.db.selectFrom('venue').select('name').execute()).toEqual([{ name: 'Tresor' }]);
	});

	it('refuses an empty name, and anything the venue lexicon refuses', async () => {
		await expect(t.venues.add(ada, { name: '  ', address: BERLIN })).rejects.toThrow("Enter the venue's name");
		await expect(t.venues.add(ada, { name: 'x'.repeat(65), address: BERLIN })).rejects.toBeInstanceOf(
			RecordValidationError
		);
		expect(await t.db.selectFrom('venue').selectAll().execute()).toEqual([]);
	});

	it('lists every venue from the start with only its public place details, until staff unlist it', async () => {
		const venue = await t.venues.add(ada, TRESOR);

		const record = listing(venue.space)!;
		expect(record).toMatchObject({
			$type: COLLECTION.listing,
			kind: 'venue',
			name: 'Tresor',
			fields: { address: TRESOR.address, geo: TRESOR.geo },
			people: []
		});
		expect(() => validateRecord(COLLECTION.listing, record)).not.toThrow();
		expect(JSON.stringify(record)).not.toMatch(/fsq|approval|slug/);

		await t.sharedSpaces.setListed(await t.viewer(ADMIN), venue.space, false);
		expect(listing(venue.space)).toBeUndefined();
	});

	it('gives back the venue with the same name in the same city and country, typed any way', async () => {
		const first = await t.venues.add(ada, { name: 'Golden  Gate', address: BERLIN });
		const names = () => t.db.selectFrom('venue').select(['name', 'locality', 'country']).execute();

		expect(await t.venues.add(ada, { name: ' golden gate', address: BERLIN })).toEqual(first);
		// Found by name, it's still the venue for a Foursquare place no venue has yet.
		expect(await t.venues.add(ada, { name: 'Golden Gate', address: BERLIN, fsq: { fsq_place_id: 'gate' } })).toEqual(
			first
		);
		expect(await names()).toHaveLength(1);

		// Names repeat: another city, another country or no city at all is another venue.
		await t.venues.add(ada, { name: 'Golden Gate', address: { country: 'DE', locality: 'Hamburg' } });
		await t.venues.add(ada, { name: 'Golden Gate', address: { country: 'AT', locality: 'Berlin' } });
		await t.venues.add(ada, { name: 'Golden Gate', address: { country: 'DE' } });
		expect(await names()).toHaveLength(4);
	});

	it('adds a venue with the same name in the same city when the two are different Foursquare places', async () => {
		const first = await t.venues.add(ada, TRESOR);
		const other = await t.venues.add(ada, { ...TRESOR, fsq: { fsq_place_id: 'another-tresor' } });

		expect(other.space).not.toBe(first.space);
		expect(other).toMatchObject({ name: 'Tresor', fsqPlaceId: 'another-tresor' });
		// Without a place to go by, the name finds one of them.
		expect([first.space, other.space]).toContain(
			(await t.venues.add(ada, { name: 'Tresor', address: TRESOR.address })).space
		);
	});

	it('lists nothing for a venue space without a venue record', async () => {
		const space = await t.sharedSpaces.create(ada, 'venue', { joins: false, listed: true });

		expect(await t.sharedSpaces.get(space)).toMatchObject({ listed: true });
		expect(listing(space)).toBeUndefined();
	});

	it('limits how many venues one account adds at a time', async () => {
		for (let i = 0; i < 30; i++) await t.venues.add(ada, { name: `Club ${i}`, address: BERLIN });

		await expect(t.venues.add(ada, { name: 'One more', address: BERLIN })).rejects.toMatchObject({ status: 429 });
	});
});
