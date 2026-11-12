import { afterEach, beforeEach, expect, it } from 'vitest';
import { sampleReport } from '#lib/mix/sample.ts';
import { CALENDAR_EVENT, COLLECTION } from '#lib/nsid.ts';
import type { ResolvedIdentity } from '#lib/server/atproto/identity.ts';
import type { JsonRecord } from '#lib/server/atproto/repo-client.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { PlyrClient } from '#lib/server/plyr.ts';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { RecordingService } from '#lib/server/services/recordings.ts';
import { ADMIN, DJ, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { callRoute, routeEvent } from '#lib/testing/route-event.ts';
import type { StrongRef } from '#lib/types.ts';
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

/** DJ goes by dj.test, verified both ways; no other account resolves. */
const identity = {
	resolve: async (did: string): Promise<ResolvedIdentity> => {
		if (did !== DJ) throw new Error(`could not resolve ${did}`);

		return { did, handle: 'dj.test', pds: 'https://pds.test', signingKey: 'did:key:z', kemKey: null };
	},
	resolveHandle: async (handle: string) => (handle === 'dj.test' ? DJ : null)
};

/** The app, with the site's public URL, the identity resolver, and sets read through the directory. */
const app = () => ({
	...f.t,
	config: { publicUrl: 'https://localnetwork.test' },
	identity,
	limits: { profileMisses: new RateLimiter(30, 30 / 600) },
	recordings: new RecordingService(
		f.t.indexedRepo,
		new PlyrClient(silentLogger, async () => new Response('{}', { status: 404 })),
		async () => true,
		silentLogger
	)
});

/** A set of DJ's, as locallibrary's `sets publish` writes it. */
const publishSet = (rkey: string, fields: JsonRecord = {}): Promise<StrongRef> =>
	f.t.net.client(DJ).putRecord({
		collection: COLLECTION.djSet,
		rkey,
		record: {
			$type: COLLECTION.djSet,
			title: 'Closing',
			date: '2026-09-01',
			venue: 'Halle Nord',
			location: { $type: 'community.lexicon.location.address', country: 'DE', locality: 'Berlin' },
			tracks: [
				{
					title: 'Night Bus',
					artists: [{ artistName: 'Ferrule', artistMbId: '1b4f1c3e-0000-4000-8000-000000000001' }]
				},
				{ unidentified: true }
			],
			createdAt: '2026-09-02T00:00:00Z',
			...fields
		}
	});

/** The page of DJ's set `rkey`, at `handle`. */
const setPage = (rkey: string, handle = 'dj.test', search = '') =>
	routeEvent({ app: app(), params: { handle, rkey }, url: `/@${handle}/sets/${rkey}${search}` });

it("shows a DJ's set under their handle, with its head and structured data for search engines", async () => {
	const set = await publishSet('3kset');
	const { loadData, locals } = setPage('3kset');
	const data = await loadData<PageServerData>(load);

	expect(data).toMatchObject({
		account: { did: DJ, handle: 'dj.test' },
		name: '@dj.test',
		set: { uri: set.uri, title: 'Closing', venue: 'Halle Nord' },
		event: null
	});
	expect(await data.mixes).toEqual([]);
	expect(data.meta).toMatchObject({
		title: 'Closing by @dj.test',
		description: '@dj.test played 2 tracks at Halle Nord, Berlin on Sep 1, 2026.',
		url: 'https://localnetwork.test/@dj.test/sets/3kset',
		type: 'music.playlist'
	});

	expect(locals).toMatchObject({
		crawlerHead: expect.stringMatching(
			new RegExp(
				[
					`<meta name="at:canonical" content="${set.uri}" />`,
					`<meta name="at:author" content="at://${DJ}" />`,
					'"@type":"MusicPlaylist","name":"Closing"'
				].join('.*')
			)
		)
	});
});

it('names the event a set was part of, when the directory keeps it', async () => {
	const event = await f.t.net.client(DJ).putRecord({
		collection: CALENDAR_EVENT,
		rkey: '3kevent',
		record: { $type: CALENDAR_EVENT, name: 'Tidal Hours', createdAt: '2026-08-01T00:00:00Z' }
	});

	await publishSet('3kset', { event: event.uri });
	await publishSet('3kelse', { event: `at://${DJ}/${CALENDAR_EVENT}/3kmissing` });

	expect(await (await setPage('3kset').loadData<PageServerData>(load)).event).toBe('Tidal Hours');
	expect(await (await setPage('3kelse').loadData<PageServerData>(load)).event).toBeNull();
});

it("links the mixes on the DJ's active projects that record the set, and their tracklists", async () => {
	const set = await publishSet('3kset');
	const tracklist = { bytes: new TextEncoder().encode(JSON.stringify(sampleReport)), mimeType: 'application/json' };

	const updated = await f.t.projectAdmin.update({
		actor: await f.t.viewer(ADMIN),
		project: f.project,
		profile: {
			...f.project.profile,
			mixes: [
				{ title: 'Late Room', url: 'https://mixes.test/late-room' },
				{ title: 'Warehouse', url: 'https://mixes.test/warehouse', set },
				{ title: 'Warehouse (radio edit)', url: 'https://mixes.test/radio', set }
			]
		},
		tracklists: [tracklist, tracklist]
	});

	const data = await setPage('3kset').loadData<PageServerData>(load);
	const project = { slug: f.project.slug, name: 'Neon Tide' };

	expect(await data.mixes).toEqual([
		{ title: 'Warehouse', url: 'https://mixes.test/warehouse', project, report: 2 },
		{ title: 'Warehouse (radio edit)', url: 'https://mixes.test/radio', project, report: null }
	]);

	// A hidden project's page doesn't open for visitors, so the set doesn't point there.
	await f.t.projectAdmin.setStatus({ actor: await f.t.viewer(ADMIN), project: updated, status: 'hidden' });
	expect(await (await setPage('3kset').loadData<PageServerData>(load)).mixes).toEqual([]);
});

it('sends a DID or a handle written another way to the set at the canonical address, keeping the query', async () => {
	await publishSet('3kset');

	await expect(
		callRoute(load, { app: app(), params: { handle: DJ, rkey: '3kset' }, url: `/@${DJ}/sets/3kset?t=1` })
	).rejects.toMatchObject({
		status: 302,
		location: '/@dj.test/sets/3kset?t=1'
	});
	await expect(
		callRoute(load, { app: app(), params: { handle: 'DJ.test', rkey: '3kset' }, url: '/@DJ.test/sets/3kset' })
	).rejects.toMatchObject({
		status: 302,
		location: '/@dj.test/sets/3kset'
	});
});

it('is a 404 for a deleted set, a key that names none, and an address that names no account', async () => {
	await publishSet('3kset');
	await f.t.net.client(DJ).deleteRecord({ collection: COLLECTION.djSet, rkey: '3kset' });

	for (const rkey of ['3kset', '3knone', '..'])
		await expect(
			callRoute(load, { app: app(), params: { handle: 'dj.test', rkey }, url: `/@dj.test/sets/${rkey}` })
		).rejects.toMatchObject({ status: 404 });
	await expect(
		callRoute(load, { app: app(), params: { handle: 'nobody.test', rkey: '3kset' }, url: '/@nobody.test/sets/3kset' })
	).rejects.toMatchObject({ status: 404 });
});
