import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { UserError } from './core';
import { silentLogger } from './logger';
import { loadPlanner } from './planner-page';
import { createTestServices } from './testing/core';
import { writtenLog } from './testing/written-log';

const ADMIN = 'did:plc:admin';

const PROMOTER = 'did:plc:promoter';

const DJ = 'did:plc:kestrel';

describe('loadPlanner', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [PROMOTER, DJ] });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const locals = (log = silentLogger) => ({ app: t, log });

	it('loads saved acts and planned shows with the acts they name', async () => {
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
		expect(Object.values(planner.performers).map((view) => view.href)).toEqual([`/projects/${project.slug}`]);
	});

	it('names a saved DJ by their profile, with an inquiry only once they take them', async () => {
		const promoter = await t.viewer(PROMOTER);
		await t.profiles.save(await t.viewer(DJ), {
			basics: { name: 'Kestrel', location: { city: 'Leeds', country: 'GB' } },
			performer: { genres: ['jungle'] },
			takesInquiries: true,
			avatar: 'keep',
			swap: null
		});
		const profile = `at://${DJ}/computer.localnetwork.profile/self`;
		await t.planner.save(promoter, profile);

		const before = await loadPlanner(locals(), promoter);
		await t.djListings.list(await t.viewer(ADMIN), DJ);
		const after = await loadPlanner(locals(), promoter);

		expect(before.performers[profile]).toMatchObject({
			name: 'Kestrel',
			genres: ['jungle'],
			place: 'Leeds, GB',
			inquiry: null
		});
		expect(after.performers[profile]).toMatchObject({ inquiry: `dj=${DJ}` });
	});

	it('still shows a saved DJ who has since turned their DJ section off, without genres', async () => {
		const promoter = await t.viewer(PROMOTER);
		await t.profiles.save(await t.viewer(DJ), { basics: { name: 'Kestrel' }, avatar: 'keep', swap: null });
		const profile = `at://${DJ}/computer.localnetwork.profile/self`;
		await t.planner.save(promoter, profile);

		const planner = await loadPlanner(locals(), promoter);

		expect(planner.performers[profile]).toMatchObject({ name: 'Kestrel', genres: [], inquiry: null });
	});

	it('leaves out a saved DJ the directory has no profile for', async () => {
		const promoter = await t.viewer(PROMOTER);
		await t.planner.save(promoter, `at://${DJ}/computer.localnetwork.profile/self`);

		const planner = await loadPlanner(locals(), promoter);

		expect(planner.saves).toHaveLength(1);
		expect(planner.performers).toEqual({});
	});

	it("is unavailable, without asking, to an account that can't keep spaces", async () => {
		const promoter = { ...(await t.viewer(PROMOTER)), spacesCapable: false };

		expect(await loadPlanner(locals(), promoter)).toEqual({ available: false, saves: [], shows: [], performers: {} });
	});

	it("says so, and logs it, when the account's server doesn't answer", async () => {
		t.net.failWrites(PROMOTER);
		const { logger, lines } = writtenLog();

		expect(await loadPlanner(locals(logger), await t.viewer(PROMOTER))).toMatchObject({
			available: false,
			problem: 'Your PDS did not respond'
		});
		expect(lines.map((line) => line.message)).toEqual(['planner unavailable']);
	});

	it('says what to do when the account has to sign in again', async () => {
		// SAFETY: the object inherits every member of the real service; only `saves` is its own.
		const planner = Object.assign(Object.create(t.planner) as typeof t.planner, {
			saves: async () => {
				throw new UserError('Sign in again to see your planner');
			}
		});

		const loaded = await loadPlanner({ app: { ...t, planner }, log: silentLogger }, await t.viewer(PROMOTER));

		expect(loaded).toMatchObject({ available: false, problem: 'Sign in again to see your planner' });
	});
});
