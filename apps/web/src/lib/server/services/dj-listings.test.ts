import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { PerformerSection } from '#lib/types.ts';
import { ForbiddenError } from '../core';
import { createTestServices, ROOT } from '../testing/core';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

describe('asking staff to list a DJ in Find DJs', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, DJ] });
		await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	/** Saves DJ's profile, with `performer` as their performer section (left off when undefined). */
	const saveProfile = async (performer: PerformerSection | undefined) => {
		const current = await t.profiles.get(DJ);

		await t.profiles.save(await t.viewer(DJ), {
			basics: { name: 'Neon', location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 } },
			performer,
			avatar: 'keep',
			swap: current?.cid ?? null
		});

		return (await t.profiles.get(DJ))!;
	};

	/** DJ's request to be listed, as they'd send it from their profile editor. */
	const ask = async () => {
		const profile = (await t.profiles.get(DJ))!;

		return t.requests.submit({
			actor: await t.viewer(DJ),
			kind: 'list',
			listing: { subject: profile.uri, cid: profile.cid },
			message: 'Please list me'
		});
	};

	/** Who Find DJs' search finds, by name. */
	const found = async () =>
		(await t.directory.query('searchPerformers')).performers.flatMap((performer) =>
			'profile' in performer ? [performer.profile.listing] : []
		);

	it('lists a DJ when staff approve, which puts them in Find DJs, and takes them out on unlisting', async () => {
		await saveProfile({ genres: ['techno'] });
		const request = await ask();

		expect(request).toMatchObject({
			kind: 'list',
			status: 'pending',
			record: { listing: { subject: `at://${DJ}/${COLLECTION.profile}/self` } }
		});
		expect(
			await t.db.selectFrom('request').select('subjectUri').where('uri', '=', request.uri).executeTakeFirst()
		).toEqual({
			subjectUri: `at://${DJ}/${COLLECTION.profile}/self`
		});
		expect(await found()).toEqual([]);

		const decided = await t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' });
		const listing = `at://${ROOT}/${COLLECTION.listingProfile}/${DJ}`;
		expect(decided.status).toBe('approved');
		expect(await t.djListings.get(DJ)).toMatchObject({ uri: listing, request: request.uri });
		expect(await found()).toEqual([listing]);

		await expect(t.djListings.unlist(await t.viewer(DJ), DJ)).rejects.toThrow(ForbiddenError);
		await t.djListings.unlist(await t.viewer(MOD), DJ);
		expect(await t.djListings.get(DJ)).toBeNull();
		expect(await found()).toEqual([]);

		await t.audit.idle();
		expect((await t.audit.list({ action: 'listing' })).map((entry) => [entry.action, entry.subject])).toEqual([
			['listing.unlist', DJ],
			['listing.list', DJ]
		]);
	});

	it('asks only for a profile with the DJ section on, once, and not when already listed', async () => {
		await saveProfile(undefined);
		await expect(ask()).rejects.toThrow('Turn on the DJ section of your profile first');

		await saveProfile({});
		const request = await ask();
		await expect(ask()).rejects.toThrow('You already asked to be listed');

		await t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' });
		await expect(ask()).rejects.toThrow("You're already listed in Find DJs");
	});

	it("won't list a DJ whose profile no longer has the DJ section on, and can't be edited like a project request", async () => {
		await saveProfile({});
		const request = await ask();
		await saveProfile(undefined);

		await expect(
			t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' })
		).rejects.toThrow('Their profile no longer has the DJ section on');
		expect(await t.djListings.get(DJ)).toBeNull();
		await expect(t.requests.revise(await t.viewer(DJ), request.uri, { message: 'again' })).rejects.toThrow(
			'Edit your profile instead'
		);
	});

	it("won't take a request naming someone else's profile", async () => {
		await saveProfile({});
		await t.profiles.save(await t.viewer(MOD), { basics: { name: 'Mo' }, performer: {}, avatar: 'keep', swap: null });

		await expect(
			t.requests.submit({
				actor: await t.viewer(MOD),
				kind: 'list',
				listing: { subject: `at://${DJ}/${COLLECTION.profile}/self`, cid: 'bafy' }
			})
		).rejects.toThrow('Set up your profile first');
	});

	it('has the directory read the profile again on listing, in case it missed it', async () => {
		await saveProfile({});
		const request = await ask();
		// As if Jetstream never brought the profile: the directory has nothing of it.
		await t.happyView.ingest({ did: DJ, collection: COLLECTION.profile, rkey: 'self', cid: '', record: null });

		await t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' });
		expect(await found()).toEqual([`at://${ROOT}/${COLLECTION.listingProfile}/${DJ}`]);
	});
});
