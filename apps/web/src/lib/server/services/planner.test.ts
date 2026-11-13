import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProjectProfile, Viewer } from '#lib/types.ts';
import { plannerSpace } from '../atproto/spaces';
import { createTestServices, ROOT } from '../testing/core';
import { PlannerService } from './planner';

const ADMIN = 'did:plc:admin';

const PROMOTER = 'did:plc:promoter';

/** An account whose PDS can't write to spaces. */
const LEGACY = 'did:plc:legacy';

const project = (rkey: string) => `at://${ROOT}/${COLLECTION.project}/${rkey}`;

const profile = (name: string): ProjectProfile => ({
	name,
	kind: 'solo',
	genres: ['house'],
	location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 }
});

describe('PlannerService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let promoter: Viewer;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [PROMOTER] });
		promoter = await t.viewer(PROMOTER);
	});
	afterEach(async () => {
		vi.useRealTimers();
		await t.audit.idle();
		await t.db.destroy();
	});

	it('asks an account whose session lapsed to sign in again', async () => {
		const lapsed = { ...(await t.viewer('did:plc:lapsed')), spacesCapable: true };

		await expect(t.planner.saves(lapsed)).rejects.toMatchObject({
			message: 'Sign in again to use your planner',
			status: 401
		});
	});

	it('reads a planner space made in an earlier run without making it again', async () => {
		await t.planner.save(promoter, project('3kept'));

		const restarted = new PlannerService(t.core);

		expect((await restarted.saves(promoter)).map((save) => save.project)).toEqual([project('3kept')]);
	});

	it('lists saves newest first, and unsaving one that was never saved writes nothing', async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime('2026-10-01T00:00:00.000Z');
		await t.planner.save(promoter, project('3first'));
		vi.setSystemTime('2026-10-02T00:00:00.000Z');
		await t.planner.save(promoter, project('3second'), 'Ask about May');

		expect(await t.planner.saves(promoter)).toMatchObject([
			{ project: project('3second'), note: 'Ask about May' },
			{ project: project('3first') }
		]);

		await t.planner.unsave(promoter, project('3never'));
		await t.audit.idle();
		expect((await t.audit.list({ action: 'planner' })).map((entry) => entry.action)).toEqual([
			'planner.save',
			'planner.save'
		]);
	});

	it('lists planned shows by date, using when it was planned for a show without one', async () => {
		const plan = (title: string, date?: string) =>
			t.planner.saveShow(promoter, { details: { title, date }, lineup: [] });

		await plan('Long ago', '2000-04-01T20:00:00.000Z');
		await plan('Undated');
		await plan('Far off', '2999-12-01T20:00:00.000Z');
		await plan('Less long ago', '2001-07-01T20:00:00.000Z');

		// Undated sorts as planned now, between the two past shows and the future one.
		expect((await t.planner.shows(promoter)).map((show) => show.details.title)).toEqual([
			'Far off',
			'Undated',
			'Less long ago',
			'Long ago'
		]);
	});

	it("finds no show it doesn't have, and plans a new one for an unknown key", async () => {
		expect(await t.planner.show(promoter, 'nope')).toBeNull();

		const rkey = await t.planner.saveShow(promoter, { rkey: 'nope', details: { title: 'Fresh' }, lineup: [] });

		expect(rkey).not.toBe('nope');
		expect(await t.planner.show(promoter, rkey)).toMatchObject({ details: { title: 'Fresh' } });
	});

	it('keeps a personal planner and refuses accounts without spaces', async () => {
		const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Saved') });
		await t.planner.save(promoter, project.uri, 'note');
		await t.planner.save(promoter, project.uri);
		expect(await t.planner.saves(promoter)).toHaveLength(1);

		const rkey = await t.planner.saveShow(promoter, {
			details: { title: 'Show' },
			lineup: [{ project: project.uri, status: 'confirmed' }]
		});

		await t.planner.saveShow(promoter, { rkey, details: { title: 'Show v2' }, lineup: [] });
		expect((await t.planner.shows(promoter)).map((s) => s.details.title)).toEqual(['Show v2']);
		await t.planner.deleteShow(promoter, rkey);
		await t.planner.unsave(promoter, project.uri);
		expect(await t.planner.saves(promoter)).toEqual([]);
		await expect(t.planner.saves(await t.viewer(LEGACY))).rejects.toThrow(/spaces/);
	});

	it('writes one save record when saves of a project overlap', async () => {
		const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Twice') });

		await Promise.all([t.planner.save(promoter, project.uri), t.planner.save(promoter, project.uri)]);

		const stored = [...t.net.spaceRecords.keys()].filter((key) => key.includes(`|${COLLECTION.plannerSave}|`));
		expect(stored).toHaveLength(1);
	});

	it('shows a project once when older saves wrote it twice, and unsaves every copy', async () => {
		const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Old') });

		const legacySave = (createdAt: string) =>
			t.net.client(PROMOTER).createSpaceRecord({
				space: plannerSpace(PROMOTER),
				collection: COLLECTION.plannerSave,
				record: { $type: COLLECTION.plannerSave, project: project.uri, createdAt }
			});

		await t.planner.saves(promoter);
		await legacySave('2026-01-01T00:00:00.000Z');
		await legacySave('2026-02-01T00:00:00.000Z');

		expect(await t.planner.saves(promoter)).toMatchObject([
			{ project: project.uri, createdAt: '2026-01-01T00:00:00.000Z' }
		]);
		await t.planner.unsave(promoter, project.uri);
		expect(await t.planner.saves(promoter)).toEqual([]);
	});

	it('creates the planner space when a fresh account loads saves and shows at once', async () => {
		// The planner page loads both in parallel, so both find no space and both try to create it.
		expect(await Promise.all([t.planner.saves(promoter), t.planner.shows(promoter)])).toEqual([[], []]);
		expect(t.net.spaces.has(plannerSpace(PROMOTER))).toBe(true);
	});

	it('surfaces a failed planner space creation and tries again on the next load', async () => {
		t.net.failWrites(PROMOTER);

		await expect(t.planner.saves(promoter)).rejects.toThrow('UpstreamFailure');
		expect(await t.planner.saves(promoter)).toEqual([]);
	});

	it('skips planner records that fail their lexicon', async () => {
		const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Kept') });
		await t.planner.save(promoter, project.uri);

		await t.net.client(PROMOTER).createSpaceRecord({
			space: plannerSpace(PROMOTER),
			collection: COLLECTION.plannerSave,
			record: { $type: COLLECTION.plannerSave, project: 42 }
		});

		expect((await t.planner.saves(promoter)).map((save) => save.project)).toEqual([project.uri]);
	});
});
