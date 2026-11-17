import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { validateRecord } from '../atproto/lexicons';
import { parseSpaceUri } from '../atproto/spaces';
import { ForbiddenError, NotFoundError, UserError } from '../core';
import { createTestServices, ROOT } from '../testing/core';

const ADMIN = 'did:plc:admin';

const ADA = 'did:plc:ada';

const BO = 'did:plc:bo';

const CY = 'did:plc:cy';

const DI = 'did:plc:di';

const basics = { name: 'Night Shift', bio: 'Techno on Thursdays.' };

const png = { upload: { bytes: new Uint8Array([1, 2, 3]), mimeType: 'image/png' } };

describe('PartyService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], flagOverrides: { spacePages: true } });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const start = async (publish = true) => t.parties.start(await t.viewer(ADA), { basics, avatar: 'keep', publish });

	const page = (space: string) => t.net.spaceRecords.get(`${space}|${ROOT}|${COLLECTION.party}|self`);

	const listing = (space: string) =>
		t.net.records.get(`${ROOT}|${COLLECTION.listing}|${parseSpaceUri(space)!.skey}`)?.value;

	const audited = async (action: string) => {
		await t.audit.idle();

		return t.db
			.selectFrom('auditEntry')
			.select(['actorDid', 'actorRole', 'summary'])
			.where('action', '=', action)
			.orderBy('seq')
			.execute();
	};

	it('starts a published party with its page, and its creator in it as its host', async () => {
		const space = await start();
		const { skey } = parseSpaceUri(space)!;

		expect(page(space)?.value).toEqual({
			$type: COLLECTION.party,
			basics,
			publishedAt: expect.any(String),
			createdAt: expect.any(String),
			updatedAt: expect.any(String)
		});
		expect(parseSpaceUri(space)).toMatchObject({ authority: ROOT, type: 'computer.localnetwork.space.party' });
		expect((await t.sharedSpaces.people(space, await t.viewer(ADA))).map(({ did, role }) => [did, role])).toEqual([
			[ADA, 'host']
		]);
		expect(await t.parties.bySkey(skey)).toEqual({
			space,
			skey,
			basics,
			avatarUrl: null,
			publishedAt: page(space)!.value.publishedAt,
			cid: page(space)!.cid
		});
		expect(await t.parties.bySpace(space)).toEqual(await t.parties.bySkey(skey));
		expect(await audited('space.create')).toEqual([
			{ actorDid: ADA, actorRole: 'member', summary: 'Started Night Shift' }
		]);
	});

	it('starts a draft with its picture, and lets two parties share a name', async () => {
		const viewer = await t.viewer(ADA);
		const draft = await t.parties.start(viewer, { basics, avatar: png, publish: false });
		const other = await start();

		expect(page(draft)?.value).not.toHaveProperty('publishedAt');
		expect(page(draft)?.value.basics).toMatchObject({ avatar: expect.objectContaining({ mimeType: 'image/png' }) });
		expect(await t.parties.bySpace(draft)).toMatchObject({
			publishedAt: null,
			avatarUrl: expect.stringMatching(new RegExp(`^/media/${ROOT}/b`))
		});
		expect(await t.parties.bySpace(other)).toMatchObject({ basics: { name: 'Night Shift' } });
		expect(await audited('space.create')).toEqual([
			{ actorDid: ADA, actorRole: 'member', summary: 'Started Night Shift as a draft' },
			{ actorDid: ADA, actorRole: 'member', summary: 'Started Night Shift' }
		]);
	});

	it('saves the page for people with page.edit and staff, over the version its form was opened on', async () => {
		const space = await start();
		const admin = await t.viewer(ADMIN);
		await t.sharedSpaces.addPerson(admin, space, { did: BO, role: 'cohost' });
		await t.sharedSpaces.addPerson(admin, space, { did: CY, role: 'door' });
		const input = { basics: { name: 'Night Shift Late' }, avatar: png, publish: false, swap: page(space)!.cid };

		await expect(t.parties.updatePage(await t.viewer(CY), space, input)).rejects.toBeInstanceOf(ForbiddenError);
		await t.parties.updatePage(await t.viewer(BO), space, input);

		const saved = page(space)!;
		expect(saved.value).toMatchObject({
			basics: { name: 'Night Shift Late', avatar: expect.objectContaining({ $type: 'blob', mimeType: 'image/png' }) }
		});
		expect(() => validateRecord(COLLECTION.party, saved.value)).not.toThrow();
		expect(await t.parties.bySpace(space)).toMatchObject({ cid: saved.cid, basics: { name: 'Night Shift Late' } });
		expect(await audited('page.update')).toEqual([
			{ actorDid: BO, actorRole: 'member', summary: "Updated Night Shift Late's page" }
		]);

		await expect(t.parties.updatePage(await t.viewer(BO), space, input)).rejects.toThrow(
			new UserError('The page changed since you opened it. Reload to see the latest, then save again.', 409)
		);

		await t.parties.updatePage(admin, space, { ...input, avatar: 'keep', swap: saved.cid });
		expect(page(space)?.value.basics).toEqual(saved.value.basics);

		await t.parties.updatePage(admin, space, { ...input, avatar: 'remove', swap: page(space)!.cid });
		expect(page(space)?.value.basics).toEqual({ name: 'Night Shift Late' });
		// Republishing a published party keeps when it was first published.
		await t.parties.updatePage(admin, space, { ...input, avatar: 'keep', publish: true, swap: page(space)!.cid });
		expect(page(space)?.value.publishedAt).toBe(saved.value.publishedAt);
	});

	it("logs a staff save with each field's before and after", async () => {
		const space = await start();

		await t.parties.updatePage(await t.viewer(ADMIN), space, {
			basics: { name: 'Night Shift', bio: 'Techno on Fridays.' },
			avatar: 'keep',
			publish: false,
			swap: page(space)!.cid
		});
		await t.audit.idle();
		const [entry] = await t.audit.list({ action: 'page.update' });

		expect(entry.actorRole).toBe('admin');
		expect(JSON.parse(entry.changes ?? '[]')).toEqual([
			{ field: 'basics.bio', before: 'Techno on Thursdays.', after: 'Techno on Fridays.' }
		]);
	});

	it('publishes a draft once, and logs it as published', async () => {
		const space = await start(false);
		const input = { basics, avatar: 'keep' as const, publish: true, swap: page(space)!.cid };

		await t.parties.updatePage(await t.viewer(ADA), space, input);

		expect(page(space)?.value.publishedAt).toEqual(expect.any(String));
		expect(await t.parties.bySpace(space)).toMatchObject({ publishedAt: page(space)!.value.publishedAt });
		expect(await audited('page.update')).toEqual([
			{ actorDid: ADA, actorRole: 'member', summary: 'Published Night Shift' }
		]);
	});

	it('refuses to save the page of a space with no party', async () => {
		const act = await t.sharedSpaces.createAct(await t.viewer(ADA), { slug: 'crew', form: 'duo', basics });
		const input = { basics, avatar: 'keep' as const, publish: false, swap: 'bafy' };

		await expect(t.parties.updatePage(await t.viewer(ADMIN), act, input)).rejects.toBeInstanceOf(NotFoundError);
	});

	it('lists nothing for a listed party space without a party record, or a draft', async () => {
		const admin = await t.viewer(ADMIN);
		const bare = await t.sharedSpaces.create(await t.viewer(ADA), 'party');
		const draft = await start(false);
		await t.sharedSpaces.setListed(admin, bare, true);
		await t.sharedSpaces.setListed(admin, draft, true);

		expect(listing(bare)).toBeUndefined();
		expect(listing(draft)).toBeUndefined();
		expect(await t.parties.bySpace(bare)).toBeNull();
		expect(await t.parties.bySkey('3nothere')).toBeNull();
	});

	it('lists a published party only once staff list it, with its hosts and cohosts who have not hidden', async () => {
		const space = await start();
		const admin = await t.viewer(ADMIN);
		await t.sharedSpaces.addPerson(admin, space, { did: BO, role: 'cohost' });
		await t.sharedSpaces.addPerson(admin, space, { did: CY, role: 'cohost', hidden: true });
		await t.sharedSpaces.addPerson(admin, space, { did: DI, role: 'door' });
		expect(listing(space)).toBeUndefined();

		await t.sharedSpaces.setListed(admin, space, true);

		const record = listing(space)!;
		expect(record).toEqual({
			$type: COLLECTION.listing,
			kind: 'party',
			name: 'Night Shift',
			fields: { basics },
			people: [
				{ did: ADA, role: 'host' },
				{ did: BO, role: 'cohost' }
			],
			updatedAt: expect.any(String)
		});
		expect(() => validateRecord(COLLECTION.listing, record)).not.toThrow();

		await t.parties.updatePage(await t.viewer(ADA), space, {
			basics: { name: 'Night Shift Late' },
			avatar: 'keep',
			publish: false,
			swap: page(space)!.cid
		});
		expect(listing(space)).toMatchObject({
			name: 'Night Shift Late',
			fields: { basics: { name: 'Night Shift Late' } }
		});
	});
});
