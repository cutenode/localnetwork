import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { Viewer } from '#lib/types.ts';
import { createTestServices, ROOT } from '../testing/core';
import { PlannerService } from './planner';

const PROMOTER = 'did:plc:promoter';

const project = (rkey: string) => `at://${ROOT}/${COLLECTION.project}/${rkey}`;

describe('PlannerService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let promoter: Viewer;

	beforeEach(async () => {
		t = await createTestServices({ capable: [PROMOTER] });
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
});
