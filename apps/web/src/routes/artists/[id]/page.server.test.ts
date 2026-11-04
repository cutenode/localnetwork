import { afterEach, beforeEach, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { SupportService } from '#lib/server/services/supports.ts';
import { DJ, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute, routeEvent } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const MBID = '2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
	// A set by Neon Tide's manager, playing a track by Ferrule, credited by MusicBrainz ID.
	await f.t.net.client(DJ).putRecord({
		collection: COLLECTION.djSet,
		rkey: '3kset1',
		record: {
			$type: COLLECTION.djSet,
			date: '2026-09-12',
			kind: 'club',
			venue: 'Halle Nord',
			tracks: [
				{ title: 'Night Bus', artist: 'Ferrule', artists: [{ artistName: 'Ferrule', artistMbId: `mbid:${MBID}` }] }
			],
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

it('shows who played an artist, with their card and structured data for search engines', async () => {
	const { loadData, locals, headers } = routeEvent({ app: app(), params: { id: MBID }, url: `/artists/${MBID}` });
	const data = await loadData<PageServerData>(load);

	expect(data.supports?.artist?.name).toBe('Ferrule');
	expect(data.meta).toMatchObject({
		title: 'Ferrule',
		url: `https://localnetwork.test/artists/${MBID}`,
		image: { url: `https://localnetwork.test/og/artist/${MBID}.png` }
	});
	expect(data.meta.noindex).toBeFalsy();
	expect(locals).toMatchObject({ crawlerHead: expect.stringContaining('musicbrainz.org/artist/' + MBID) });
	expect(headers['cache-control']).toBe('public, max-age=60');
});

it('sends an ID written another way to its canonical address', async () => {
	const id = `mbid:${MBID.toUpperCase()}`;

	await expect(callRoute(load, { app: app(), params: { id }, url: `/artists/${id}` })).rejects.toMatchObject({
		status: 308,
		location: `/artists/${MBID}`
	});
});

it('is a 404 for an address that is no MusicBrainz ID', async () => {
	await expect(
		callRoute(load, { app: app(), params: { id: 'ferrule' }, url: '/artists/ferrule' })
	).rejects.toMatchObject({ status: 404 });
});

it('keeps an artist no set credits out of search, without structured data', async () => {
	const other = '5e6f7a8b-9c0d-4e1f-a2b3-c4d5e6f7a8b9';
	const { loadData, locals } = routeEvent({ app: app(), params: { id: other }, url: `/artists/${other}` });
	const data = await loadData<PageServerData>(load);

	expect(data.supports?.artist).toBeFalsy();
	expect(data.meta).toMatchObject({ title: 'Artist', noindex: true });
	expect(locals).not.toHaveProperty('crawlerHead');
});

it("renders without the artist's supports while the directory is offline", async () => {
	const directory = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response('Service Unavailable', { status: 503 })
	});

	const supports = new SupportService(directory, f.t.accounts, silentLogger);

	const data = await loadRoute<PageServerData>(load, {
		app: { ...app(), supports },
		params: { id: MBID },
		url: `/artists/${MBID}`
	});

	expect(data.supports).toBeNull();
	expect(data.meta).toMatchObject({ title: 'Artist', noindex: true });
});
