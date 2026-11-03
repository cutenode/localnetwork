import pino from 'pino';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { loadPlanner } from './planner-page';
import { createTestServices } from './testing/core';

const ADMIN = 'did:plc:admin';

const PROMOTER = 'did:plc:promoter';

describe('loadPlanner', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let lines: string[];

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [PROMOTER] });
		lines = [];
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const locals = () => ({
		app: { planner: t.planner, projects: t.projects },
		log: pino({}, { write: (line: string) => void lines.push(line) })
	});

	it('loads saved projects and planned shows with the projects they name', async () => {
		const project = await t.projectAdmin.create({
			actor: await t.viewer(ADMIN),
			profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
		});

		const promoter = await t.viewer(PROMOTER);
		await t.planner.save(promoter, project.uri);
		await t.planner.saveShow(promoter, { details: { title: 'Winter Social' }, lineup: [{ project: project.uri }] });

		const planner = await loadPlanner(locals(), promoter);

		expect(planner.available).toBe(true);
		expect(planner.saves.map((save) => save.project)).toEqual([project.uri]);
		expect(planner.shows.map((show) => show.details.title)).toEqual(['Winter Social']);
		expect(Object.values(planner.projects).map((view) => view.slug)).toEqual([project.slug]);
	});

	it("is unavailable, without asking, to an account that can't keep spaces", async () => {
		const promoter = { ...(await t.viewer(PROMOTER)), spacesCapable: false };

		expect(await loadPlanner(locals(), promoter)).toEqual({ available: false, saves: [], shows: [], projects: {} });
	});

	it("says so, and logs it, when the account's server doesn't answer", async () => {
		t.net.failWrites(PROMOTER);

		expect(await loadPlanner(locals(), await t.viewer(PROMOTER))).toMatchObject({
			available: false,
			problem: 'Your PDS did not respond'
		});
		expect(lines.join('')).toContain('"msg":"planner unavailable"');
	});
});
