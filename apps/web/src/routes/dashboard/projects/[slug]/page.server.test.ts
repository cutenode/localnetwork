import { afterEach, beforeEach, expect, it } from 'vitest';
import { CALENDAR_EVENT } from '#lib/nsid.ts';
import type { AccountService } from '#lib/server/services/accounts.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { IndexedRepo } from '#lib/server/happyview/indexed.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { DjService } from '#lib/server/services/dj.ts';
import type { EpkService } from '#lib/server/services/epk.ts';
import { ShowService } from '#lib/server/services/shows.ts';
import { CODJ, DJ, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

/** Next year, so the show stays upcoming. */
const YEAR = new Date().getUTCFullYear() + 1;

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The app as the project dashboard reads it, with DJs' records read through `dj`. */
const app = (dj = new DjService(f.t.indexedRepo, silentLogger)) => ({ ...f.t, dj });

/** The app, with press kits and accounts that may only offer what the dashboard reads of them. */
type Services = Omit<ReturnType<typeof app>, 'epks' | 'accounts'> & {
	epks: Pick<EpkService, 'dashboard'>;
	accounts: Pick<AccountService, 'views'>;
};

/** The project dashboard's load for DJ (with `handle`), under the layout that found the project. */
const loaded = async (services: Services = app(), handle: string | null = null) =>
	loadRoute<PageServerData>(load, {
		app: services,
		viewer: { ...(await f.t.viewer(DJ)), handle },
		url: `/dashboard/projects/${f.project.slug}`,
		params: { slug: f.project.slug },
		parent: { project: f.project }
	});

/** DJ announces a show for the project. */
const announce = async () =>
	f.t.shows.save(await f.t.viewer(DJ), f.project, {
		name: 'Tidal Hours',
		date: `${YEAR}-11-07`,
		start: '23:00',
		location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 },
		cancelled: false
	});

it("shows the project's managers, press kits, pending changes and shows, marking shows another app edited", async () => {
	await f.t.accounts.recordLogin({
		did: CODJ,
		handle: 'codj.test',
		pdsUrl: null,
		grantedScope: 'atproto',
		spacesCapable: true,
		publicWrites: true
	});

	const epk = await f.publish(DJ);

	const pending = await f.t.requests.submit({
		actor: await f.t.viewer(DJ),
		kind: 'update',
		projectUri: f.project.uri,
		details: { bookingEmail: 'book@neon.test' }
	});

	const show = await announce();
	const rkey = show.uri.split('/').at(-1)!;
	const event = f.t.net.records.get(`${DJ}|${CALENDAR_EVENT}|${rkey}`)!.value;
	await f.t.net.client(DJ).putRecord({ collection: CALENDAR_EVENT, rkey, record: { ...event, name: 'Sold out' } });

	const data = await loaded();

	expect(data.managers).toEqual([
		{ did: DJ, handle: null },
		{ did: CODJ, handle: 'codj.test' }
	]);
	expect(data.pendingUpdate?.uri).toBe(pending.uri);
	expect((await data.kits)?.kits.map((kit) => kit.epk.uri)).toEqual([epk.uri]);
	expect((await data.shows)?.map((listed) => listed.uri)).toEqual([show.uri]);
	expect(await data.edited).toEqual([show.uri]);
	expect(await data.stats).toEqual([]);
});

it("shows a DJ's stats without waiting once they were read in the last few minutes", async () => {
	const dj = new DjService(f.t.indexedRepo, silentLogger);

	const first = await loaded(app(dj));
	expect(first.stats).toBeInstanceOf(Promise);
	await first.stats;

	expect((await loaded(app(dj))).stats).toEqual([]);
});

it('shows no shows, and none edited, while HappyView is down', async () => {
	await announce();

	const down = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response('down', { status: 503 })
	});

	const data = await loaded({
		...app(),
		shows: new ShowService(f.t.core, f.t.projects, down, new IndexedRepo(down, silentLogger, 0), 0)
	});

	expect(await data.shows).toBeNull();
	expect(await data.edited).toEqual([]);
});

it('leaves the press kit summary out when the press kits cannot be read', async () => {
	const epks = {
		dashboard: async () => {
			throw new Error('database is locked');
		}
	};

	const data = await loaded({ ...app(), epks });

	expect(await data.kits).toBeNull();
	expect(data.managers).toHaveLength(2);
});
