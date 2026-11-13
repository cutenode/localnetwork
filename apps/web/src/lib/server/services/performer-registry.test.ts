import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { performerUri } from '#lib/server/performer-uri.ts';
import type { ProjectProfile } from '#lib/types.ts';
import { createTestServices, ROOT } from '../testing/core';

const ADMIN = 'did:plc:admin';

const ADA = 'did:plc:ada';

const BO = 'did:plc:bo';

const CY = 'did:plc:cy';

const profile: ProjectProfile = {
	name: 'Old Crew',
	kind: 'duo',
	genres: ['house'],
	location: { city: 'Berlin', country: 'DE' }
};

describe('PerformerService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN] });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const startAct = async () =>
		t.sharedSpaces.createAct(await t.viewer(ADA), { slug: 'crew', form: 'duo', basics: { name: 'Crew' } });

	/** A performer space Ada keeps under her own name, its record written by the root and synced back. */
	const djSpace = async (formerly?: string[]) => {
		const space = await t.sharedSpaces.create(await t.viewer(ADA), 'performer');
		const at = new Date().toISOString();
		const subject = { $type: `${COLLECTION.performer}#dj`, did: ADA };
		const value = { $type: COLLECTION.performer, subject, ...(formerly && { formerly }), createdAt: at, updatedAt: at };
		await t.net.client(ROOT).putSpaceRecord({ space, collection: COLLECTION.performer, rkey: 'self', record: value });
		await t.sharedSpaces.index({ space, collection: COLLECTION.performer, rkey: 'self', value });

		return space;
	};

	const skeyOf = (space: string) => space.split('/').at(-1)!;

	it('finds an act by the URI that names it, its slug and its space key', async () => {
		const space = await startAct();
		const uri = performerUri(ROOT, { kind: 'act', skey: skeyOf(space) });
		const act = await t.performerRegistry.byUri(uri);

		expect(act).toMatchObject({ kind: 'act', uri, space, act: { slug: 'crew', basics: { name: 'Crew' } } });
		expect(await t.performerRegistry.bySlug('crew')).toEqual(act);
		expect(await t.performerRegistry.byKey(skeyOf(space))).toEqual(act);
		expect(await t.performerRegistry.bySlug('nobody')).toBeNull();
		// A listing in anyone else's repo names nothing.
		expect(await t.performerRegistry.byUri(`at://${BO}/${COLLECTION.listing}/${skeyOf(space)}`)).toBeNull();
	});

	it("finds a DJ's own space by their profile's URI, and the performer a project became", async () => {
		const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile, managers: [BO] });
		const space = await djSpace([project.uri]);
		const uri = performerUri(ROOT, { kind: 'dj', did: ADA });

		expect(await t.performerRegistry.forDj(ADA)).toMatchObject({
			kind: 'dj',
			uri,
			space,
			did: ADA,
			formerly: [project.uri]
		});
		expect(await t.performerRegistry.byUri(uri)).toMatchObject({ kind: 'dj', space });
		expect(await t.performerRegistry.aliases(project.uri)).toMatchObject({ kind: 'dj', space });
		expect(await t.performerRegistry.aliases(`at://${ROOT}/${COLLECTION.project}/other`)).toBeNull();
		expect(await t.performerRegistry.forDj(BO)).toBeNull();

		// The project itself, until callers move to what it became.
		expect(await t.performerRegistry.byUri(project.uri)).toMatchObject({
			kind: 'project',
			uri: project.uri,
			project: { slug: project.slug }
		});
	});

	it('lists who holds a capability, leaving hidden people out', async () => {
		const admin = await t.viewer(ADMIN);
		const space = await startAct();
		const act = { kind: 'act' as const, skey: skeyOf(space) };
		await t.sharedSpaces.addPerson(admin, space, { did: BO, role: 'manager', hidden: true });
		await t.sharedSpaces.addPerson(admin, space, { did: CY, role: 'manager' });

		expect(await t.performerRegistry.holders(act, 'bookings.answer')).toEqual([ADA, CY]);
		expect(await t.performerRegistry.holders(act, 'people.manage')).toEqual([ADA]);
		expect(await t.performerRegistry.holders({ kind: 'act', skey: 'nothing' }, 'page.edit')).toEqual([]);
	});

	it('counts a DJ as holding every capability for themselves, and every project manager for their project', async () => {
		const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile, managers: [BO, CY] });
		const space = await djSpace();
		await t.sharedSpaces.addPerson(await t.viewer(ADMIN), space, { did: CY, role: 'manager' });

		expect(await t.performerRegistry.holders({ kind: 'dj', did: ADA }, 'shows.manage')).toEqual([ADA, CY]);
		expect(await t.performerRegistry.holders({ kind: 'dj', did: BO }, 'shows.manage')).toEqual([BO]);
		expect(await t.performerRegistry.holders({ kind: 'project', rkey: project.rkey }, 'people.manage')).toEqual([
			BO,
			CY
		]);
		expect(await t.performerRegistry.holders({ kind: 'project', rkey: 'gone' }, 'people.manage')).toEqual([]);
	});
});
