import { afterEach, beforeEach, expect, it } from 'vitest';
import { sampleReport } from '#lib/mix/sample.ts';
import { COLLECTION } from '#lib/nsid.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { PlyrClient } from '#lib/server/plyr.ts';
import { RecordingService } from '#lib/server/services/recordings.ts';
import { ADMIN, DJ, pressKitServices, STRANGER } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute, routeEvent } from '#lib/testing/route-event.ts';
import type { ProjectView, StrongRef, Viewer } from '#lib/types.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

let project: ProjectView;

let set: StrongRef;

beforeEach(async () => {
	f = await pressKitServices();

	set = await f.t.net.client(DJ).putRecord({
		collection: COLLECTION.djSet,
		rkey: '3kset',
		record: {
			$type: COLLECTION.djSet,
			title: 'Closing',
			date: '2026-09-01',
			tracks: [{}],
			createdAt: '2026-09-02T00:00:00Z'
		}
	});

	const tracklist = { bytes: new TextEncoder().encode(JSON.stringify(sampleReport)), mimeType: 'application/json' };

	// Two mixes with a tracklist, the second linking a set, and one without.
	project = await f.t.projectAdmin.update({
		actor: await f.t.viewer(ADMIN),
		project: f.project,
		profile: {
			...f.project.profile,
			mixes: [
				{ title: 'Late Room', url: 'https://mixes.test/late-room' },
				{ title: 'Warehouse', url: 'https://mixes.test/warehouse', set },
				{ title: 'No tracklist', url: 'https://mixes.test/plain' }
			]
		},
		tracklists: [tracklist, tracklist]
	});
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The app, with the site's public URL, the sets mixes link, and a plyr.fm that has no tracks. */
const app = () => ({
	...f.t,
	config: { publicUrl: 'https://localnetwork.test' },
	sets: f.t.projects.sets,
	recordings: new RecordingService(
		f.t.indexedRepo,
		new PlyrClient(silentLogger, async () => new Response('{}', { status: 404 })),
		async () => true,
		silentLogger
	)
});

/** The page of the `n`th mix of `slug`, as `viewer` loads it. */
const mixPage = (n: string, viewer: Viewer | null = null, slug = project.slug) =>
	loadRoute<PageServerData>(load, { app: app(), viewer, params: { slug, n }, url: `/projects/${slug}/mixes/${n}` });

it("keeps a signed-in viewer's page out of shared caches", async () => {
	const { loadData, headers } = routeEvent({
		app: app(),
		viewer: await f.t.viewer('did:plc:someone'),
		params: { slug: project.slug, n: '1' },
		url: `/projects/${project.slug}/mixes/1`
	});

	await loadData<PageServerData>(load);

	expect(headers['cache-control']).toBe('private, max-age=300');
});

it("shows a mix's tracklist, its player and its page's head, cached for five minutes", async () => {
	const { loadData, headers, locals } = routeEvent({
		app: app(),
		params: { slug: project.slug, n: '1' },
		url: `/projects/${project.slug}/mixes/1`
	});

	const data = await loadData<PageServerData>(load);

	expect(data).toMatchObject({
		project: { slug: project.slug, name: 'Neon Tide', status: 'active' },
		mix: { title: 'Late Room', url: 'https://mixes.test/late-room' },
		n: 1,
		sets: undefined
	});
	expect(data.report.plays).toHaveLength(sampleReport.plays.length);
	expect(await data.source).toBeNull();
	expect(data.meta.url).toBe(`https://localnetwork.test/projects/${project.slug}/mixes/1`);
	expect(headers['cache-control']).toBe('public, max-age=300');
	expect(locals).toMatchObject({ crawlerHead: expect.stringContaining(project.uri) });
});

it('streams the set a mix links, and names it in the head', async () => {
	const { loadData, locals } = routeEvent({
		app: app(),
		params: { slug: project.slug, n: '2' },
		url: `/projects/${project.slug}/mixes/2`
	});

	const data = await loadData<PageServerData>(load);

	expect([...((await data.sets)?.keys() ?? [])]).toEqual([set.uri]);
	expect(locals).toMatchObject({ crawlerHead: expect.stringContaining(set.uri) });
});

it("is a 404 for a mix that isn't there or has no tracklist", async () => {
	for (const n of ['0', '1.5', 'x', '3', '4'])
		await expect(mixPage(n)).rejects.toMatchObject({ status: 404, body: { message: expect.any(String) } });
	await expect(mixPage('1', null, 'nobody')).rejects.toMatchObject({ status: 404 });
});

it("shows a hidden project's mix to its managers and staff, uncached, and to no one else", async () => {
	await f.t.projectAdmin.setStatus({ actor: await f.t.viewer(ADMIN), project, status: 'hidden' });

	const { loadData, headers } = routeEvent({
		app: app(),
		viewer: await f.t.viewer(DJ),
		params: { slug: project.slug, n: '1' },
		url: `/projects/${project.slug}/mixes/1`
	});

	expect((await loadData<PageServerData>(load)).project.status).toBe('hidden');
	expect(headers).toEqual({});
	expect((await mixPage('1', await f.t.viewer(ADMIN))).meta.noindex).toBe(true);
	await expect(mixPage('1', await f.t.viewer(STRANGER))).rejects.toMatchObject({ status: 404 });
	await expect(mixPage('1')).rejects.toMatchObject({ status: 404 });
});

it("answers with the tracklist's own error when it can't be fetched", async () => {
	const cid = project.profile.mixes?.[0]?.tracklist?.ref.$link ?? '';

	f.t.net.blobs.delete(cid);

	await expect(mixPage('1')).rejects.toMatchObject({
		status: 502,
		body: { message: 'This tracklist could not be fetched right now.' }
	});
});

it('lets an unexpected failure through as it is', async () => {
	const failing = new Error('database is locked');

	const requests = {
		joinedManagers: async () => {
			throw failing;
		}
	};

	await expect(
		callRoute(load, {
			app: { ...app(), requests },
			params: { slug: project.slug, n: '1' },
			url: `/projects/${project.slug}/mixes/1`
		})
	).rejects.toBe(failing);
});
