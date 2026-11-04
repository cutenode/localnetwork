import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { SupportService } from '#lib/server/services/supports.ts';
import { ADMIN, DJ, FRIEND, PROMOTER, pressKitServices } from '#lib/server/testing/press-kits.ts';
import type { Charts } from '#lib/types.ts';
import { loadRoute, routeEvent } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	vi.useFakeTimers({ toFake: ['Date'] });
	vi.setSystemTime(new Date('2026-09-20T12:00:00Z'));
	f = await pressKitServices();
	const admin = await f.t.viewer(ADMIN);

	// Two more acts besides Neon Tide, so a track all three play has the DJs it takes to chart.
	for (const [did, name] of [
		[FRIEND, 'Low Arc'],
		[PROMOTER, 'Kiln']
	]) {
		const created = await f.t.projectAdmin.create({
			actor: admin,
			profile: { name, kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
		});

		await f.t.projectAdmin.setManagers({ actor: admin, project: created, managers: [did] });
	}

	// A set by each act in Berlin, eight days ago, playing a track by Ferrule.
	for (const did of [DJ, FRIEND, PROMOTER])
		await f.t.net.client(did).putRecord({
			collection: COLLECTION.djSet,
			rkey: '3kset1',
			record: {
				$type: COLLECTION.djSet,
				date: '2026-09-12',
				kind: 'club',
				venue: 'Halle Nord',
				location: {
					$type: 'community.lexicon.location.address',
					name: 'Halle Nord',
					locality: 'Berlin',
					country: 'DE'
				},
				tracks: [{ title: 'Night Bus', artist: 'Ferrule' }],
				createdAt: '2026-10-01T00:00:00Z'
			}
		});
});

afterEach(async () => {
	vi.useRealTimers();
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The app, with the site's public URL. */
const app = () => ({ ...f.t, config: { publicUrl: 'https://localnetwork.test' } });

/** The artists a chart names. */
const artists = (charts: Charts | null) => charts?.artists.map((artist) => artist.name);

it('charts the last 30 days everywhere, with the past year of months to pick from', async () => {
	const { loadData, headers } = routeEvent({ app: app(), url: '/charts' });
	const data = await loadData<PageServerData>(load);

	expect(data.filters).toMatchObject({
		month: null,
		place: null,
		query: { since: '2026-08-22', before: '2026-09-21' }
	});
	expect(data.months).toHaveLength(12);
	expect(data.months.slice(0, 2)).toEqual(['2026-09', '2026-08']);
	expect(artists(await data.charts)).toEqual(['Ferrule']);
	expect(data.meta.url).toBe('https://localnetwork.test/charts');
	expect(headers['cache-control']).toBe('public, max-age=60');
});

it('charts a month and a place picked in the address', async () => {
	const berlin = await loadRoute<PageServerData>(load, { app: app(), url: '/charts?month=2026-09&place=Berlin,%20de' });
	const august = await loadRoute<PageServerData>(load, { app: app(), url: '/charts?month=2026-08' });

	expect(berlin.filters).toMatchObject({ month: '2026-09', place: 'Berlin, DE' });
	expect(artists(await berlin.charts)).toEqual(['Ferrule']);
	expect(artists(await august.charts)).toEqual([]);
});

it('streams null for the charts while the directory is offline, so the filters still show', async () => {
	const directory = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response('Service Unavailable', { status: 503 })
	});

	const supports = new SupportService(directory, f.t.accounts, silentLogger);
	const data = await loadRoute<PageServerData>(load, { app: { ...app(), supports }, url: '/charts' });

	expect(await data.charts).toBeNull();
});
