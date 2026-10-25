import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { requireManager } from './manage';
import { createTestServices } from './testing/core';

const ADMIN = 'did:plc:admin';

const DJ = 'did:plc:dj';

const FAN = 'did:plc:fan';

describe('requireManager', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN] });
		const admin = await t.viewer(ADMIN);

		const created = await t.projectAdmin.create({
			actor: admin,
			profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
		});

		await t.projectAdmin.setManagers({ actor: admin, project: created, managers: [DJ] });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const url = new URL('https://x.test/dashboard/projects/neon-tide');

	const check = async (did: string | null, slug = 'neon-tide') => {
		const locals = { viewer: did ? await t.viewer(did) : null, app: { projects: t.projects } };

		return requireManager(locals, url, slug).then(
			({ project }) => project.slug,
			(cause: unknown) => cause
		);
	};

	it("lets a project's managers in and turns everyone else away", async () => {
		expect(await check(DJ)).toBe('neon-tide');
		// Staff manage projects from the staff console, not here.
		expect(await check(ADMIN)).toMatchObject({ status: 403 });
		expect(await check(FAN)).toMatchObject({
			status: 403,
			body: { message: "Only this project's managers can manage it" }
		});
		expect(await check(DJ, 'nobody')).toMatchObject({ status: 404 });
		expect(await check(null)).toMatchObject({
			status: 303,
			location: '/login?next=%2Fdashboard%2Fprojects%2Fneon-tide'
		});
	});
});
