import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProjectProfile } from '#lib/types.ts';
import { recordCid } from '../atproto/repo-client';
import { createTestServices, ROOT } from '../testing/core';

const ADMIN = 'did:plc:admin';

const BERLIN = { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 };

const profile = (name: string): ProjectProfile => ({ name, kind: 'solo', genres: ['techno'], location: BERLIN });

describe('ProjectService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN] });
	});
	afterEach(async () => {
		vi.restoreAllMocks();
		await t.audit.idle();
		await t.db.destroy();
	});

	const publish = (name: string, details?: null) =>
		t.projects.publish({ profile: profile(name), managers: [], status: 'active', createdBy: ADMIN, details });

	/** A project record as the root writes it, named `name` and managed by `managers`. */
	const projectRecord = (name: string, managers: string[] = []) => ({
		$type: COLLECTION.project,
		slug: name.toLowerCase().slice(0, 48),
		profile: { name, kind: 'solo', genres: ['techno'], location: BERLIN },
		managers,
		status: 'active',
		revision: 1,
		createdBy: ROOT,
		createdAt: '2026-10-30T00:00:00.000Z',
		updatedAt: '2026-10-30T00:00:00.000Z'
	});

	/** A project record the root published that the lexicon doesn't allow (its name is too long), which the directory keeps. */
	const publishInvalid = () =>
		t.net
			.client(ROOT)
			.putRecord({ collection: COLLECTION.project, rkey: '3bad', record: projectRecord('Bad'.repeat(100)) });

	it('leaves projects the lexicon refuses out of searches and matches', async () => {
		await publish('Good');
		await publishInvalid();

		const found = await t.projects.search({ sort: 'name' });
		expect(found.total).toBe(2);
		expect(found.items.map((project) => project.profile.name)).toEqual(['Good']);

		const matches = await t.projects.match({ location: { lat: 52.52, lng: 13.405 }, radiusKm: 50, genres: ['techno'] });
		expect(matches.map((match) => match.candidate.profile.name)).toEqual(['Good']);
	});

	it("reconciles every valid project's spaces, skipping a record the lexicon refuses", async () => {
		await publishInvalid();

		await expect(t.projects.reconcileManagerSpaces()).resolves.toBeUndefined();
	});

	it('reads every page of a search that runs past one', async () => {
		const managers = Array.from({ length: 61 }, (_, index) => `did:plc:m${index}`);

		// Stored as they are, since the record script would keep them unchanged and 61 writes only slow the test.
		t.happyView.seed(
			await Promise.all(
				managers.map(async (did, index) => {
					const record = projectRecord(`Project ${index}`, [did]);

					return {
						did: ROOT,
						collection: COLLECTION.project,
						rkey: `3p${index}`,
						cid: await recordCid(record),
						record
					};
				})
			)
		);

		expect((await t.projects.managers()).toSorted()).toEqual(managers.toSorted());
	});

	it('saves empty private details when told to clear them, and has none for a project never given any', async () => {
		const { project } = await publish('Cleared', null);
		const { project: plain } = await publish('Plain');

		expect(await t.projects.details(project.uri)).toEqual({});
		expect(await t.projects.details(plain.uri)).toBeNull();
	});

	it('reads only project records of the root from its repo', async () => {
		expect(await t.projects.current(`at://${ADMIN}/${COLLECTION.project}/3abc`)).toBeNull();
		expect(await t.projects.current('not a uri')).toBeNull();
	});

	it('passes on a write the PDS fails for any reason but a stale version', async () => {
		t.net.failWrites(ROOT);

		await expect(publish('Down')).rejects.toThrow('UpstreamFailure');
		expect((await t.projects.search({})).items).toEqual([]);
	});

	it('falls back to a random suffix once the first thousand slugs for a name are taken', async () => {
		// Every slug asked about is taken.
		vi.spyOn(t.directory, 'query').mockResolvedValue({
			project: { uri: `at://${ROOT}/${COLLECTION.project}/3echo`, cid: 'bafyecho', record: {} }
		});

		expect(await t.projects.uniqueSlug('Echo')).toMatch(/^echo-[a-z2-7]{13}$/);
	});
});
