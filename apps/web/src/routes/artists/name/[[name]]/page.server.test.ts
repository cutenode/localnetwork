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
	// A set by Neon Tide's manager: Ferrule without an MBID, and Ostgut Ton credited by one.
	await f.t.net.client(DJ).putRecord({
		collection: COLLECTION.djSet,
		rkey: '3kset1',
		record: {
			$type: COLLECTION.djSet,
			date: '2026-09-12',
			kind: 'club',
			venue: 'Halle Nord',
			tracks: [
				{ title: 'Night Bus', artist: 'Ferrule' },
				{
					title: 'Dawn Chorus',
					artist: 'Ostgut Ton',
					artists: [{ artistName: 'Ostgut Ton', artistMbId: `mbid:${MBID}` }]
				}
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

it('shows an artist without an MBID by the name in the path, as its canonical address', async () => {
	const { loadData, headers } = routeEvent({ app: app(), params: { name: 'Ferrule' }, url: '/artists/name/Ferrule' });
	const data = await loadData<PageServerData>(load);

	expect(data.supports?.artist?.name).toBe('Ferrule');
	expect(data.meta).toMatchObject({ title: 'Ferrule', url: 'https://localnetwork.test/artists/name/Ferrule' });
	expect(data.meta.noindex).toBeFalsy();
	expect(headers['cache-control']).toBe('public, max-age=60');
});

it("names the MBID page as canonical when the name's plays carry one", async () => {
	const data = await loadRoute<PageServerData>(load, {
		app: app(),
		params: { name: 'Ostgut Ton' },
		url: '/artists/name/Ostgut%20Ton'
	});

	expect(data.meta.url).toBe(`https://localnetwork.test/artists/${MBID}`);
});

it("reads a name the path can't carry from the query, and keeps a name no set credits out of search", async () => {
	const data = await loadRoute<PageServerData>(load, { app: app(), url: '/artists/name?name=..' });

	expect(data.supports?.artist).toBeFalsy();
	expect(data.meta).toMatchObject({
		title: '..',
		url: 'https://localnetwork.test/artists/name?name=..',
		noindex: true
	});
});

it('sends a blank name back to the artist search', async () => {
	await expect(callRoute(load, { app: app(), url: '/artists/name' })).rejects.toMatchObject({
		status: 307,
		location: '/artists'
	});
	await expect(
		callRoute(load, { app: app(), params: { name: '  ' }, url: '/artists/name/%20%20' })
	).rejects.toMatchObject({ status: 307 });
});

it('names the page after the address while the directory is offline', async () => {
	const directory = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response('Service Unavailable', { status: 503 })
	});

	const supports = new SupportService(directory, f.t.accounts, silentLogger);

	const data = await loadRoute<PageServerData>(load, {
		app: { ...app(), supports },
		params: { name: 'Ferrule' },
		url: '/artists/name/Ferrule'
	});

	expect(data.supports).toBeNull();
	expect(data.meta).toMatchObject({ title: 'Ferrule', noindex: true });
});
