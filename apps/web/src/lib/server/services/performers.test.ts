import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProjectProfile } from '#lib/types.ts';
import { createTestServices, ROOT } from '../testing/core';

const ADMIN = 'did:plc:admin';

const DJ = 'did:plc:dj';

/** A DJ whose profile the directory keeps but the profile lexicon doesn't allow. */
const LONG_NAME = 'did:plc:longname';

const BERLIN = { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 };

const profile = (name: string): ProjectProfile => ({ name, kind: 'solo', genres: ['techno'], location: BERLIN });

describe('PerformerSearch', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, DJ, LONG_NAME] });
		const admin = await t.viewer(ADMIN);

		await t.projectAdmin.create({ actor: admin, profile: profile('Crew') });
		await t.accounts.recordLogin({
			did: DJ,
			handle: 'dj.test',
			pdsUrl: null,
			grantedScope: 'atproto',
			spacesCapable: true,
			publicWrites: true
		});
		await t.profiles.save(await t.viewer(DJ), {
			basics: { name: 'Neon', location: BERLIN },
			performer: { genres: ['techno'] },
			avatar: 'keep',
			swap: null
		});
		await t.net.client(LONG_NAME).putRecord({
			collection: COLLECTION.profile,
			rkey: 'self',
			record: {
				$type: COLLECTION.profile,
				basics: { name: 'n'.repeat(65), location: BERLIN },
				performer: {},
				createdAt: '2026-10-30T00:00:00.000Z',
				updatedAt: '2026-10-30T00:00:00.000Z'
			}
		});

		for (const did of [DJ, LONG_NAME]) await t.djListings.list(admin, did);
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	it('finds projects and listed DJs together, with each DJ handle, leaving out what the lexicons refuse', async () => {
		const { items, total } = await t.performers.search({ sort: 'name' });

		expect(total).toBe(3);
		expect(items).toMatchObject([
			{ kind: 'project', project: { profile: { name: 'Crew' } } },
			{ kind: 'dj', dj: { profile: { did: DJ }, handle: 'dj.test', genres: ['techno'] } }
		]);
		expect(items).toHaveLength(2);
		expect(items.find((item) => item.kind === 'dj')).not.toHaveProperty('dj.distanceKm');
	});

	it('gives each DJ distance from a place searched near', async () => {
		const { items } = await t.performers.search({ near: { lat: 52.52, lng: 13.405 }, radiusKm: 50 });

		expect(items.find((item) => item.kind === 'dj')).toMatchObject({ dj: { distanceKm: 0 } });
	});

	it("leaves out a project record the root published that the lexicon doesn't allow", async () => {
		await t.net.client(ROOT).putRecord({
			collection: COLLECTION.project,
			rkey: '3bad',
			record: {
				$type: COLLECTION.project,
				slug: 'bad',
				profile: { name: 'Bad'.repeat(100), kind: 'solo', genres: ['techno'], location: BERLIN },
				managers: [],
				status: 'active',
				revision: 1,
				createdBy: ROOT,
				createdAt: '2026-10-30T00:00:00.000Z',
				updatedAt: '2026-10-30T00:00:00.000Z'
			}
		});

		const { items, total } = await t.performers.search({ sort: 'name' });

		expect(total).toBe(4);
		expect(items.map((item) => (item.kind === 'project' ? item.project.profile.name : item.dj.profile.did))).toEqual([
			'Crew',
			DJ
		]);
	});
});
