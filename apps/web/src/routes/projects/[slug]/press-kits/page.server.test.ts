import { afterEach, beforeEach, expect, it } from 'vitest';
import { silentLogger } from '#lib/server/logger.ts';
import { DjService } from '#lib/server/services/dj.ts';
import { ADMIN, DJ, pressKitServices, STRANGER } from '#lib/server/testing/press-kits.ts';
import { loadRoute, routeEvent, type RouteEventInit } from '#lib/testing/route-event.ts';
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

/** The list page's event for `did` (signed out without one), with the services the project page reads. */
const listPage = async (did: string | null): Promise<RouteEventInit> => ({
	app: {
		...f.t,
		dj: new DjService(f.t.indexedRepo, silentLogger),
		config: { publicUrl: 'https://localnetwork.test' }
	},
	viewer: did ? await f.t.viewer(did) : null,
	url: `/projects/${f.project.slug}/press-kits`,
	params: { slug: f.project.slug }
});

/** What the list page loads for `did`. */
const pressKits = async (did: string | null) => loadRoute<PageServerData>(load, await listPage(did));

it("lists a project's press kits for anyone, with the profile's full address", async () => {
	await f.publish(DJ, { title: 'Festivals' });
	const data = await pressKits(null);

	expect(data).toMatchObject({
		project: { slug: f.project.slug },
		details: null,
		isManager: false,
		profileUrl: `https://localnetwork.test/projects/${f.project.slug}`,
		meta: { title: 'Neon Tide press kit', noindex: false }
	});
	expect(await data.epks).toMatchObject([{ record: { title: 'Festivals' } }]);
});

it("writes the project's record tags into the head for crawlers", async () => {
	const page = routeEvent(await listPage(null));

	await page.call(load);

	expect(page.locals).toMatchObject({
		crawlerHead: expect.stringContaining(`<meta name="at:canonical" content="${f.project.uri}" />`)
	});
});

it("leaves a hidden project's list out of search, and shows it only to its managers and staff", async () => {
	await f.t.projectAdmin.setStatus({ actor: await f.t.viewer(ADMIN), project: f.project, status: 'hidden' });

	await expect(pressKits(STRANGER)).rejects.toMatchObject({ status: 404 });

	const page = routeEvent(await listPage(DJ));
	const data = await page.loadData<PageServerData>(load);

	expect(data).toMatchObject({ isManager: true, meta: { noindex: true } });
	expect(data.details).toEqual({});
	expect(page.locals).not.toHaveProperty('crawlerHead');
});
