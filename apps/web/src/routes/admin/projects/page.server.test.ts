import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { ProjectProfile } from '#lib/types.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

/** A solo techno project in Berlin called `name`. */
const profile = (name: string): ProjectProfile => ({
	name,
	kind: 'solo',
	genres: ['techno'],
	location: { city: 'Berlin', country: 'DE' }
});

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
	const admin = await t.viewer(ADMIN);
	await t.staff.grant(admin, MOD, 'moderator');

	await t.projectAdmin.create({ actor: admin, profile: profile('Neon Tide'), managers: [DJ] });
	const hidden = await t.projectAdmin.create({ actor: admin, profile: profile('Deep Current') });
	await t.projectAdmin.setStatus({ actor: admin, project: hidden, status: 'hidden' });
	await t.projectAdmin.create({ actor: admin, profile: profile('Iron Pulse') });
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The project list as the moderator sees it at `url`. */
const projects = async (url: string) => loadRoute<PageServerData>(load, { app: t, viewer: await t.viewer(MOD), url });

/** The names on a page of the list. */
const names = (data: Awaited<ReturnType<typeof projects>>) => data.items.map((item) => item.profile.name);

it('lists every project by name, hidden ones included', async () => {
	const data = await projects('/admin/projects');

	expect(names(data)).toEqual(['Deep Current', 'Iron Pulse', 'Neon Tide']);
	expect(data).toMatchObject({ total: 3, page: 1, pageSize: 60, q: '', status: '', unclaimed: false });
});

it('searches by name, and filters by status and by projects nobody manages', async () => {
	expect(names(await projects('/admin/projects?q=pulse'))).toEqual(['Iron Pulse']);

	const hidden = await projects('/admin/projects?status=hidden');
	expect(names(hidden)).toEqual(['Deep Current']);
	expect(hidden.status).toBe('hidden');

	const unclaimed = await projects('/admin/projects?unclaimed');
	expect(names(unclaimed)).toEqual(['Deep Current', 'Iron Pulse']);
	expect(unclaimed.unclaimed).toBe(true);
});

it('ignores a status it does not know and a page that is not a number', async () => {
	const data = await projects('/admin/projects?status=deleted&page=last');

	expect(data).toMatchObject({ total: 3, page: 1, status: '' });
});

it('turns to a later page', async () => {
	expect((await projects('/admin/projects?page=2')).items).toEqual([]);
});

it('refuses an account without a role', async () => {
	const viewer = await t.viewer(DJ);

	await expect(loadRoute<PageServerData>(load, { app: t, viewer, url: '/admin/projects' })).rejects.toMatchObject({
		status: 403
	});
});
