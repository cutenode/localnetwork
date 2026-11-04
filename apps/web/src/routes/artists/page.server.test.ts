import { afterEach, beforeEach, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { SupportService } from '#lib/server/services/supports.ts';
import { DJ, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { loadRoute, routeEvent } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
	// A set by Neon Tide's manager, playing a track by Ferrule.
	await f.t.net.client(DJ).putRecord({
		collection: COLLECTION.djSet,
		rkey: '3kset1',
		record: {
			$type: COLLECTION.djSet,
			date: '2026-09-12',
			kind: 'club',
			venue: 'Halle Nord',
			tracks: [{ title: 'Night Bus', artist: 'Ferrule' }],
			createdAt: '2026-10-01T00:00:00Z'
		}
	});
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The app, with the site's public URL. */
const app = () => ({ ...f.t, config: { publicUrl: 'https://localnetwork.test' } });

/** A directory that answers every query with a 503, as HappyView does while it's down. */
const offline = new HappyViewClient({
	url: 'http://happyview.test',
	fetch: async () => new Response('Service Unavailable', { status: 503 })
});

it('finds the artists DJs played, and lets a shared cache keep the page while signed out', async () => {
	const { loadData, headers } = routeEvent({ app: app(), url: '/artists?q=%20Ferrule%20' });
	const data = await loadData<PageServerData>(load);

	expect(data.q).toBe('Ferrule');
	expect((await data.artists)?.map((artist: { name: string }) => artist.name)).toEqual(['Ferrule']);
	expect(data.meta.url).toBe('https://localnetwork.test/artists');
	expect(headers).toEqual({ 'cache-control': 'public, max-age=60', vary: 'Cookie' });
});

it('searches with an empty query when none is given', async () => {
	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/artists' });

	expect(data.q).toBe('');
	expect(await data.artists).toEqual([expect.objectContaining({ name: 'Ferrule' })]);
});

it('streams null for the artists while the directory is offline, so the search box still shows', async () => {
	const supports = new SupportService(offline, f.t.accounts, silentLogger);
	const data = await loadRoute<PageServerData>(load, { app: { ...app(), supports }, url: '/artists?q=Ferrule' });

	expect(await data.artists).toBeNull();
});
