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

/** The app as the home page reads it, with `projects` in place of the directory's own. */
const app = (projects: ProjectService = f.t.projects) => ({
	...f.t,
	projects,
	geocoder: { enabled: true },
	config: { publicUrl: 'https://localnetwork.computer' }
});

it('shows the newest projects and the directory totals, with the tool cards and no captures', async () => {
	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/' });

	expect(data.recent.map((project) => project.slug)).toEqual([f.project.slug]);
	expect(data.stats).toEqual({ active: 1, cities: 1, unclaimed: 0 });
	expect(data.genres).toEqual([expect.objectContaining({ value: 'techno', count: 1 })]);
	expect(data.cities).toHaveLength(1);
	expect(data.geocoder).toBe(true);
	expect(data.tools.map((tool) => tool.id)).toEqual(['locallibrary', 'localtoolbox']);
	expect(Object.keys(data.tools[0]).sort()).toEqual(['id', 'kind', 'lines', 'name', 'tagline']);
	expect(data.meta).toMatchObject({ url: 'https://localnetwork.computer/' });
});

it('keeps only the top 14 genres and 8 cities', async () => {
	const admin = await f.t.viewer(ADMIN);

	for (let n = 0; n < 16; n++) {
		await f.t.projectAdmin.create({
			actor: admin,
			profile: { name: `DJ ${n}`, kind: 'solo', genres: [`genre-${n}`], location: { city: `City ${n}`, country: 'DE' } }
		});
	}

	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/' });

	expect(data.genres).toHaveLength(14);
	expect(data.cities).toHaveLength(8);
});

it('still opens without the directory while HappyView is down', async () => {
	const offline = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response('Service Unavailable', { status: 503 })
	});

	const projects = new ProjectService(f.t.core, offline);
	const data = await loadRoute<PageServerData>(load, { app: app(projects), url: '/' });

	expect(data).toMatchObject({ recent: [], stats: null, genres: [], cities: [] });
	expect(data.tools).toHaveLength(2);
});

it('fails the page on any other error', async () => {
	// SAFETY: the object inherits every member of the real service; only `recent` is its own.
	const broken = Object.assign(Object.create(f.t.projects) as ProjectService, {
		recent: async () => {
			throw new TypeError('broken');
		}
	});

	await expect(loadRoute<PageServerData>(load, { app: app(broken), url: '/' })).rejects.toThrow('broken');
});
