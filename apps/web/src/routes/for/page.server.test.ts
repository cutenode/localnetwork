import { afterEach, beforeEach, expect, it } from 'vitest';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { ProjectService } from '#lib/server/services/projects.ts';
import { ADMIN, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

const app = (projects: ProjectService = f.t.projects) => ({
	...f.t,
	projects,
	config: { publicUrl: 'https://localnetwork.test' }
});

it("describes the page for search engines and link previews at the site's own address", async () => {
	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/for' });

	expect(data.meta).toMatchObject({ title: 'Who Local Network is for', url: 'https://localnetwork.test/for' });
});

it("streams the directory's totals", async () => {
	const admin = await f.t.viewer(ADMIN);
	await f.t.projectAdmin.create({
		actor: admin,
		profile: { name: 'Halflight', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } }
	});

	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/for' });

	// The fixture's own project has a manager; the one an admin just listed is up for claiming.
	expect(await data.stats).toEqual({ djs: 2, cities: 2, unclaimed: 1 });
});

it('still opens while HappyView is down', async () => {
	const offline = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response('Service Unavailable', { status: 503 })
	});

	const data = await loadRoute<PageServerData>(load, { app: app(new ProjectService(f.t.core, offline)), url: '/for' });

	expect(await data.stats).toBeNull();
});
