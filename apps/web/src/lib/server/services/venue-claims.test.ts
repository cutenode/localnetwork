import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { NotFoundError, UserError } from '../core';
import { createTestServices, ROOT } from '../testing/core';
import type { VenueView } from './venues';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const ADA = 'did:plc:ada';

const BO = 'did:plc:bo';

const evidence = { relationship: 'owner', evidence: 'The venue site lists me as its owner.' };

describe('venue claims', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let venue: VenueView;
	let mod: Viewer;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, ADA, BO] });
		await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
		mod = await t.viewer(MOD);
		venue = await t.venues.add(await t.viewer(BO), { name: 'Tresor', address: { country: 'DE', locality: 'Berlin' } });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const claim = async (did = ADA, space = venue.space) =>
		t.requests.submit({ actor: await t.viewer(did), kind: 'claim', space, claim: evidence });

	const person = (did: string) => t.net.spaceRecords.get(`${venue.space}|${ROOT}|${COLLECTION.person}|${did}`)?.value;

	it('files a claim on the venue for staff, with its evidence', async () => {
		const request = await claim();

		expect(request).toMatchObject({
			kind: 'claim',
			status: 'pending',
			projectUri: null,
			project: null,
			venue: { key: venue.skey, name: 'Tresor' },
			record: { kind: 'claim', space: venue.space, claim: evidence }
		});
		expect(() => validateRecord(COLLECTION.request, { ...request.record })).not.toThrow();
		expect(await t.requests.list({ kind: 'claim', status: 'pending' })).toEqual([request]);

		await t.audit.idle();

		const entry = await t.db
			.selectFrom('auditEntry')
			.selectAll()
			.where('action', '=', 'request.submit')
			.executeTakeFirst();

		expect(entry?.summary).toBe('Submitted a claim for Tresor');
	});

	it('refuses a second open claim, a claim with no evidence, and a venue that is not there', async () => {
		await claim();

		await expect(claim()).rejects.toThrow(new UserError('You already have a claim waiting for review on this venue'));
		await expect(
			t.requests.submit({
				actor: await t.viewer(BO),
				kind: 'claim',
				space: venue.space,
				claim: { relationship: 'owner', evidence: ' ' }
			})
		).rejects.toThrow(new UserError('Tell the moderators how to verify your claim'));
		await expect(claim(BO, `${venue.space}x`)).rejects.toBeInstanceOf(NotFoundError);
	});

	it('adds the claimant to the venue as an owner when staff approve, through their claim', async () => {
		const request = await claim();

		const decided = await t.requests.decide({ actor: mod, uri: request.uri, outcome: 'approved' });

		expect(decided.status).toBe('approved');
		expect(person(ADA)).toMatchObject({ subject: ADA, role: 'owner', via: 'claim', source: request.uri, addedBy: MOD });
		expect(() => validateRecord(COLLECTION.person, person(ADA)!)).not.toThrow();
		expect(t.net.spaces.get(venue.space)?.members.get(ADA)).toMatchObject({ read: true });
		expect((await t.sharedSpaces.people(venue.space, mod)).map(({ did, role }) => [did, role])).toEqual([
			[ADA, 'owner']
		]);

		// Owning it now, they can't claim it again.
		await expect(claim()).rejects.toThrow(new UserError('You already own this venue'));
	});

	it('makes someone already in the venue, like a booking manager, an owner', async () => {
		await t.sharedSpaces.addPerson(await t.viewer(ADMIN), venue.space, { did: ADA, role: 'booker' });
		const request = await claim();

		await t.requests.decide({ actor: mod, uri: request.uri, outcome: 'approved' });

		expect(person(ADA)).toMatchObject({
			role: 'owner',
			capabilities: ['page.edit', 'people.manage', 'venue.approve', 'venue.block']
		});
	});

	it('adds nobody when staff decline, and takes a new claim after', async () => {
		const first = await claim();

		await t.requests.decide({ actor: mod, uri: first.uri, outcome: 'rejected', reason: "We couldn't verify it" });

		expect(person(ADA)).toBeUndefined();
		const again = await claim();
		expect(again.status).toBe('pending');
		expect(again.uri).not.toBe(first.uri);
	});

	it('takes revised evidence after staff ask for changes', async () => {
		const request = await claim();
		await t.requests.decide({ actor: mod, uri: request.uri, outcome: 'changesRequested', reason: 'Add a link' });

		const revised = await t.requests.revise(await t.viewer(ADA), request.uri, {
			claim: { ...evidence, links: ['https://tresor.example/team'] }
		});

		expect(revised).toMatchObject({
			status: 'pending',
			record: { space: venue.space, claim: { links: ['https://tresor.example/team'] } }
		});
	});

	it('refuses to approve once the claimant owns it, or once the venue is gone', async () => {
		const request = await claim();
		await t.sharedSpaces.addPerson(await t.viewer(ADMIN), venue.space, { did: ADA, role: 'owner' });

		await expect(t.requests.decide({ actor: mod, uri: request.uri, outcome: 'approved' })).rejects.toThrow(
			new UserError('They already own this venue')
		);

		const other = await claim(BO);
		await t.db.deleteFrom('venue').execute();
		await expect(t.requests.decide({ actor: mod, uri: other.uri, outcome: 'approved' })).rejects.toThrow(
			new UserError('The venue no longer exists')
		);
		// The queue still shows it, without the venue's name.
		expect((await t.requests.get(other.uri))?.venue).toBeNull();
	});
});
