import { cidForLex } from '@atproto/lex-cbor';
import { type JsonObject, jsonToLex } from '@atproto/lex-json';
import { parseLexiconDoc } from '@atproto/lexicon';
import { describe, expect, it } from 'vitest';
import { COLLECTION, DIRECTORY } from '#lib/nsid.ts';
import { KNOWN_GENRES } from '#lib/taxonomy/genres.ts';
import type { DjSetView, ProjectRecord, ShowRecord, StrongRef } from '#lib/types.ts';
import { lexicons } from '../atproto/lexicons';
import { FakeHappyView } from '../testing/happyview';
import { happyViewBundle } from './bundle';
import type { DirectoryMatch, DirectoryOutputs, DirectoryProject, DirectoryShow } from './client';

const ROOT = 'did:plc:root';

const BERLIN = { latE6: 52_520_000, lngE6: 13_405_000 };

const POTSDAM = { latE6: 52_390_600, lngE6: 13_064_500 };

const MUNICH = { latE6: 48_135_100, lngE6: 11_582_000 };

const HAMBURG = { latE6: 53_551_000, lngE6: 9_993_000 };

/** The client's output types, with the project record typed so tests can read its fields. */
type View = Omit<DirectoryProject, 'record'> & { record: ProjectRecord };

type Match = Omit<DirectoryMatch, 'project'> & { project: View };

type Page = Omit<DirectoryOutputs['searchProjects'], 'projects'> & { projects: View[] };

type Params = Record<string, string | number | boolean | string[]>;

/** Calls a directory query the way an XRPC client does, and checks the answer against the query's lexicon. */
async function query<T>(hv: FakeHappyView, name: keyof typeof DIRECTORY, params: Params = {}): Promise<T> {
	const search = Object.entries(params)
		.flatMap(([key, value]) => [value].flat().map((item) => `${key}=${encodeURIComponent(item)}`))
		.join('&');

	const response = await hv.fetch(`http://happyview.test/xrpc/${DIRECTORY[name]}?${search}`);
	const body: unknown = await response.json();
	expect({ status: response.status, body }).toMatchObject({ status: 200 });
	lexicons.assertValidXrpcOutput(DIRECTORY[name], body);

	// SAFETY: assertValidXrpcOutput just checked `body` against the query's lexicon, which each caller's T mirrors.
	return body as T;
}

const names = (page: { projects: View[] }) => page.projects.map((project) => project.record.profile.name);

let published = 0;

/**
 * Delivers a project record like Jetstream would, from the root account unless `did` says otherwise, with `profile`
 * and `record` fields over valid defaults. Each one is a minute newer than the last. Returns its URI.
 */
async function publish(
	hv: FakeHappyView,
	profile: JsonObject = {},
	record: JsonObject = {},
	options: { did?: string; rkey?: string } = {}
): Promise<string> {
	const { did, rkey, value } = projectRecord(profile, record, options);
	const cid = (await cidForLex(jsonToLex(value))).toString();
	await hv.ingest({ did, collection: COLLECTION.project, rkey, cid, record: value });

	return `at://${did}/${COLLECTION.project}/${rkey}`;
}

/** A valid project record with `profile` and `record` fields over the defaults, a minute newer than the last. */
function projectRecord(
	profile: JsonObject = {},
	record: JsonObject = {},
	options: { did?: string; rkey?: string } = {}
) {
	published += 1;
	const { did = ROOT, rkey = `3k${String(published).padStart(11, '0')}` } = options;
	const createdAt = new Date(Date.UTC(2026, 0, 1) + published * 60_000).toISOString();

	const value = {
		slug: `project-${published}`,
		profile: {
			name: `Project ${published}`,
			kind: 'solo',
			genres: ['techno'],
			location: { city: 'Berlin', country: 'DE', ...BERLIN },
			...profile
		},
		managers: [],
		status: 'active',
		revision: 1,
		createdBy: ROOT,
		createdAt,
		updatedAt: createdAt,
		...record
	};

	return { did, rkey, value };
}

describe('record script', () => {
	it('indexes valid project records from the root account only', async () => {
		// The queries only read the root account's records anyway, so a probe shows what got stored at all.
		const bundle = happyViewBundle(ROOT);

		const hv = new FakeHappyView(ROOT, {
			lexicons: [
				...bundle.lexicons,
				parseLexiconDoc({ lexicon: 1, id: 'test.fake.stored', defs: { main: { type: 'query' } } })
			],
			scripts: [
				...bundle.scripts,
				{
					id: 'xrpc.query:test.fake.stored',
					body: "function handle() return db.raw('SELECT uri FROM happyview_records') end"
				}
			]
		});

		await publish(hv, { name: 'Squatter' }, {}, { did: 'did:plc:stranger' });
		const paused = await publish(hv, { name: 'Paused' }, { status: 'paused' });
		const placeless = await publish(hv, { location: { city: 'Berlin' } });
		const stringly = await publish(hv, { capacity: { min: '100' } });
		const valid = await publish(hv, { name: 'Valid' });

		expect(await (await hv.fetch('http://happyview.test/xrpc/test.fake.stored')).json()).toEqual([{ uri: valid }]);
		expect(hv.logs).toEqual([
			`skipped ${paused}: bad slug, status, or managers`,
			`skipped ${placeless}: bad place`,
			`skipped ${stringly}: bad capacity`
		]);
	});

	it('replaces a project on update and removes it on delete', async () => {
		const hv = new FakeHappyView(ROOT);
		const uri = await publish(hv, { name: 'Before' }, {}, { rkey: 'kept' });
		await publish(hv, { name: 'After' }, {}, { rkey: 'kept' });
		expect(names(await query<Page>(hv, 'searchProjects'))).toEqual(['After']);

		await hv.ingest({ did: ROOT, collection: COLLECTION.project, rkey: 'kept', cid: '', record: null });
		expect(await query(hv, 'getProject', { uri })).toEqual({});
	});
});

describe('getProject, getProjects, and getStats', () => {
	it('finds a project by URI or slug, in any status', async () => {
		const hv = new FakeHappyView(ROOT);
		const first = await publish(hv, { name: 'First' }, { slug: 'echo', status: 'hidden' });
		await publish(hv, { name: 'Second' }, { slug: 'echo' });

		expect(await query(hv, 'getProject', { uri: first })).toMatchObject({
			project: { uri: first, cid: expect.any(String) }
		});
		expect(await query(hv, 'getProject', { slug: 'echo' })).toMatchObject({
			project: { record: { profile: { name: 'First' } } }
		});
		expect(await query(hv, 'getProject', { slug: 'nope' })).toEqual({});
		expect(await query(hv, 'getProject')).toEqual({});
	});

	it('returns projects in the order asked for, once each, without unknown ones', async () => {
		const hv = new FakeHappyView(ROOT);
		const a = await publish(hv, { name: 'A' }, { status: 'archived' });
		const b = await publish(hv, { name: 'B' });
		const missing = `at://${ROOT}/${COLLECTION.project}/missing`;

		expect(names(await query<Page>(hv, 'getProjects', { uris: [b, missing, a, b] }))).toEqual(['B', 'A']);
		expect(names(await query<Page>(hv, 'getProjects', { uris: a }))).toEqual(['A']);
	});

	it('counts projects and lists the genres and cities of active ones, most common first', async () => {
		const hv = new FakeHappyView(ROOT);
		const empty = await query(hv, 'getStats');
		expect(empty).toEqual({ total: 0, active: 0, unclaimed: 0, cityCount: 0, genres: [], cities: [] });

		await publish(hv, { genres: ['techno', 'house'] }, { managers: ['did:plc:dj'] });
		await publish(hv, { genres: ['house'], location: { city: 'Hamburg', country: 'de' } });
		await publish(hv, { genres: ['ambient'], location: { city: 'Leipzig', country: 'DE' } }, { status: 'hidden' });

		expect(await query(hv, 'getStats')).toEqual({
			total: 3,
			active: 2,
			unclaimed: 2,
			cityCount: 3,
			genres: [
				{ value: 'house', count: 2 },
				{ value: 'techno', count: 1 }
			],
			cities: [
				{ city: 'Berlin', country: 'DE', count: 1 },
				{ city: 'Hamburg', country: 'DE', count: 1 }
			]
		});
	});
});

type Sets = { sets: (DjSetView & { tracks?: unknown })[] };

type Records = DirectoryOutputs['listRecords'];

/** Delivers any record like Jetstream would, with its CID worked out like a PDS does. Returns its URI and CID. */
async function ingest(hv: FakeHappyView, did: string, collection: string, rkey: string, record: JsonObject) {
	const value = { $type: collection, ...record };
	const cid = (await cidForLex(jsonToLex(value))).toString();
	await hv.ingest({ did, collection, rkey, cid, record: value });

	return { uri: `at://${did}/${collection}/${rkey}`, cid };
}

let played = 0;

/**
 * Delivers a dj.set record like Jetstream would, with `fields` over valid defaults, each played a day after the
 * last. Returns its URI and CID.
 */
async function publishSet(hv: FakeHappyView, did: string, fields: JsonObject = {}): Promise<StrongRef> {
	played += 1;
	const rkey = `3s${String(played).padStart(11, '0')}`;

	const value = {
		$type: COLLECTION.djSet,
		date: new Date(Date.UTC(2026, 0, played)).toISOString().slice(0, 10),
		tracks: [{ title: 'One', artist: 'A' }, { unidentified: true }],
		createdAt: '2026-10-01T00:00:00.000Z',
		...fields
	};

	const cid = (await cidForLex(jsonToLex(value))).toString();
	await hv.ingest({ did, collection: COLLECTION.djSet, rkey, cid, record: value });

	return { uri: `at://${did}/${COLLECTION.djSet}/${rkey}`, cid };
}

describe('set record script', () => {
	it("keeps every account's sets and serves those by the root's projects' managers, without their tracks", async () => {
		const hv = new FakeHappyView(ROOT);
		// A project in someone else's repo doesn't make its managers count.
		await publish(hv, {}, { managers: ['did:plc:faker'] }, { did: 'did:plc:stranger' });
		await publish(hv, {}, { managers: ['did:plc:dj'], status: 'hidden' });

		const kept = await publishSet(hv, 'did:plc:dj', {
			title: 'Closing',
			kind: 'club',
			venue: 'Tresor',
			location: {
				$type: 'community.lexicon.location.address',
				country: 'DE',
				locality: 'Berlin',
				street: 'Köpenicker'
			},
			// Only locallibrary reads these, through getRecords, so setViews leave them out.
			sources: [{ kind: 'history' }]
		});

		const bare = await publishSet(hv, 'did:plc:dj', { location: { $type: 'other.place', name: 'Somewhere' } });
		await publishSet(hv, 'did:plc:faker');
		await publishSet(hv, 'did:plc:nobody');
		const broken = await publishSet(hv, 'did:plc:dj', { date: 20260101 });

		const { sets } = await query<Sets>(hv, 'listSets', { actors: ['did:plc:dj', 'did:plc:faker', 'did:plc:nobody'] });

		expect(sets).toEqual([
			{ ...bare, date: '2026-01-02', trackCount: 2 },
			{
				...kept,
				title: 'Closing',
				date: '2026-01-01',
				kind: 'club',
				venue: 'Tresor',
				location: {
					$type: 'community.lexicon.location.address',
					country: 'DE',
					locality: 'Berlin',
					street: 'Köpenicker'
				},
				trackCount: 2
			}
		]);
		expect(hv.logs).toEqual([`skipped ${broken.uri}: bad date or tracks`]);
		// Kept for when they come to manage a project, and the support queries leave them out until then.
		expect(
			(await query<Records>(hv, 'listRecords', { actor: 'did:plc:nobody', collection: COLLECTION.djSet })).records
		).toHaveLength(1);
		expect((await query<Sets>(hv, 'getSets', { uris: kept.uri })).sets).toHaveLength(1);
	});

	it('keeps what the DJ sets block and locallibrary read: the tracklist, people, links, sources and address as written', async () => {
		const hv = new FakeHappyView(ROOT);

		const tracks = [
			{ title: 'One', artist: 'A', recordingMbId: `mbid:${mbid(1)}`, bpmCentis: 12_800, key: '8A', startSeconds: 0 },
			{ unidentified: true, bpmCentis: 13_000, key: '9A', transitionIn: { technique: 'blend', harmony: 'adjacent' } }
		];

		const set = await publishSet(hv, 'did:plc:dj', {
			tracks,
			playedWith: [{ name: 'Two Rooms' }],
			links: [{ uri: 'https://mixes.test/one', name: 'Mixcloud' }],
			event: 'at://did:plc:dj/community.lexicon.calendar.event/one',
			sources: [{ kind: 'history' }],
			location: address('Berlin')
		});

		const [record] = (await query<Records>(hv, 'getRecords', { uris: set.uri })).records;

		expect(record).toEqual({
			...set,
			value: expect.objectContaining({
				tracks,
				playedWith: [{ name: 'Two Rooms' }],
				links: [{ uri: 'https://mixes.test/one', name: 'Mixcloud' }],
				event: 'at://did:plc:dj/community.lexicon.calendar.event/one',
				sources: [{ kind: 'history' }],
				location: address('Berlin'),
				createdAt: '2026-10-01T00:00:00.000Z',
				trackCount: 2
			})
		});
	});

	it('removes a deleted set', async () => {
		const hv = new FakeHappyView(ROOT);
		await publish(hv, {}, { managers: ['did:plc:dj'] });
		const set = await publishSet(hv, 'did:plc:dj');
		const rkey = set.uri.split('/').at(-1)!;

		await hv.ingest({ did: 'did:plc:dj', collection: COLLECTION.djSet, rkey, cid: '', record: null });
		expect((await query<Sets>(hv, 'getSets', { uris: [set.uri] })).sets).toEqual([]);
	});
});

describe('getSets and listSets', () => {
	it('returns sets in the order asked for, once each, without unknown ones', async () => {
		const hv = new FakeHappyView(ROOT);
		await publish(hv, {}, { managers: ['did:plc:a', 'did:plc:b'] });
		const a = await publishSet(hv, 'did:plc:a', { title: 'A' });
		const b = await publishSet(hv, 'did:plc:b', { title: 'B' });
		const missing = `at://did:plc:a/${COLLECTION.djSet}/missing`;

		const { sets } = await query<Sets>(hv, 'getSets', { uris: [b.uri, missing, a.uri, b.uri] });
		expect(sets.map((set) => set.title)).toEqual(['B', 'A']);
	});

	it('lists only the accounts asked for, most recently played first, up to the limit', async () => {
		const hv = new FakeHappyView(ROOT);
		await publish(hv, {}, { managers: ['did:plc:a', 'did:plc:b', 'did:plc:c'] });

		for (const title of ['A1', 'B1', 'C1', 'A2', 'B2'])
			await publishSet(hv, `did:plc:${title[0].toLowerCase()}`, { title });

		const titles = async (params: Params) => (await query<Sets>(hv, 'listSets', params)).sets.map((set) => set.title);

		expect(await titles({ actors: ['did:plc:a', 'did:plc:b'] })).toEqual(['B2', 'A2', 'B1', 'A1']);
		expect(await titles({ actors: 'did:plc:c' })).toEqual(['C1']);
		expect(await titles({ actors: ['did:plc:a', 'did:plc:b'], limit: 1 })).toEqual(['B2']);
	});
});

describe('getRecords and listRecords', () => {
	it('returns records of any collection in the order asked for, once each, without unknown ones', async () => {
		const hv = new FakeHappyView(ROOT);
		const project = await publish(hv, { name: 'Neon Tide' });
		const set = await publishSet(hv, 'did:plc:dj');
		const missing = `at://did:plc:dj/${COLLECTION.djSet}/missing`;

		const { records } = await query<Records>(hv, 'getRecords', { uris: [set.uri, missing, project, set.uri] });

		expect(records.map((record) => record.uri)).toEqual([set.uri, project]);
		expect(records[1]).toMatchObject({ cid: expect.any(String), value: { profile: { name: 'Neon Tide' } } });
	});

	it("lists one account's records of one collection, last record key first, a page at a time", async () => {
		const hv = new FakeHappyView(ROOT);
		const sets = [];

		for (let i = 0; i < 5; i++) sets.push((await publishSet(hv, 'did:plc:dj')).uri);
		await publishSet(hv, 'did:plc:other');
		await ingest(hv, 'did:plc:dj', COLLECTION.djProfile, 'self', { generatedAt: '2026-10-01T00:00:00.000Z' });

		const page = async (params: Params = {}) => {
			const result = await query<Records>(hv, 'listRecords', {
				actor: 'did:plc:dj',
				collection: COLLECTION.djSet,
				...params
			});

			return { uris: result.records.map((record) => record.uri), cursor: result.cursor };
		};

		const first = await page({ limit: 2 });
		expect(first.uris).toEqual([sets[4], sets[3]]);
		const second = await page({ limit: 2, cursor: first.cursor! });
		expect(second.uris).toEqual([sets[2], sets[1]]);
		expect(await page({ limit: 2, cursor: second.cursor! })).toEqual({ uris: [sets[0]], cursor: undefined });
		expect((await page()).uris).toEqual(sets.toReversed());
	});
});

describe('DJ profile, rekordbox settings, and press kit record scripts', () => {
	const SETTINGS = 'computer.localnetwork.locallibrary.settings';

	it("keeps every account's DJ stats and rekordbox settings as written, at the key self only", async () => {
		const hv = new FakeHappyView(ROOT);
		const profile = { generatedAt: '2026-10-01T00:00:00.000Z', tempo: { low: 120, high: 132, median: 126 } };
		const settings = { updatedAt: '2026-10-01T00:00:00.000Z', player: { quantize: 'on' } };
		const kept = [];
		const uris = [];

		for (const [collection, record] of [
			[COLLECTION.djProfile, profile],
			[SETTINGS, settings]
		] as const) {
			kept.push({
				...(await ingest(hv, 'did:plc:anyone', collection, 'self', record)),
				value: { $type: collection, ...record }
			});
			uris.push(kept.at(-1)!.uri, (await ingest(hv, 'did:plc:anyone', collection, 'other', record)).uri);
		}

		expect((await query<Records>(hv, 'getRecords', { uris })).records).toEqual(kept);

		await hv.ingest({ did: 'did:plc:anyone', collection: SETTINGS, rkey: 'self', cid: '', record: null });
		expect((await query<Records>(hv, 'getRecords', { uris })).records).toEqual(kept.slice(0, 1));
	});

	it('keeps press kits as written, by an account that manages the root project they name', async () => {
		const hv = new FakeHappyView(ROOT);
		const project = await publish(hv, {}, { managers: ['did:plc:dj'] });
		// A project in someone else's repo doesn't make its managers count.
		const fake = await publish(hv, {}, { managers: ['did:plc:faker'] }, { did: 'did:plc:stranger' });

		const epk = (fields: JsonObject = {}) => ({
			project,
			title: 'Clubs',
			access: { anyone: 'review' },
			createdAt: '2026-10-01T00:00:00.000Z',
			...fields
		});

		const kept = await ingest(hv, 'did:plc:dj', COLLECTION.epk, 'kept', epk());

		const uris = [
			kept.uri,
			(await ingest(hv, 'did:plc:nobody', COLLECTION.epk, 'theirs', epk())).uri,
			(await ingest(hv, 'did:plc:faker', COLLECTION.epk, 'fake', epk({ project: fake }))).uri,
			(await ingest(hv, 'did:plc:dj', COLLECTION.epk, 'odd', epk({ project: 7 }))).uri
		];

		expect((await query<Records>(hv, 'getRecords', { uris })).records).toEqual([
			{ ...kept, value: { $type: COLLECTION.epk, ...epk() } }
		]);

		await hv.ingest({ did: 'did:plc:dj', collection: COLLECTION.epk, rkey: 'kept', cid: '', record: null });
		expect((await query<Records>(hv, 'getRecords', { uris })).records).toEqual([]);
	});
});

type Charts = DirectoryOutputs['getCharts'];

type Artist = DirectoryOutputs['getArtist'];

const address = (locality: string) => ({ $type: 'community.lexicon.location.address', country: 'DE', locality });

/** An active project for each of `djs` (did:plc:<dj>), so each counts as a DJ of their own. */
async function acts(hv: FakeHappyView, ...djs: string[]) {
	for (const dj of djs) await publish(hv, {}, { managers: [`did:plc:${dj}`] });
}

/** A MusicBrainz ID that's easy to tell apart in a test. */
const mbid = (n: number) => `bead0000-cafe-4000-8000-${String(n).padStart(12, '0')}`;

describe('getCharts', () => {
	it('counts a DJ once per track, charts tracks three DJs support, and counts only those for artists', async () => {
		const hv = new FakeHappyView(ROOT);
		await acts(hv, 'a', 'b', 'c');
		const hit = { title: 'Hit', artist: 'Ferrule' };
		const pair = { title: 'Pair', artist: 'Ferrule' };
		await publishSet(hv, 'did:plc:a', { tracks: [hit, pair, hit] });
		await publishSet(hv, 'did:plc:a', { tracks: [hit] });
		await publishSet(hv, 'did:plc:b', { tracks: [hit, pair] });
		await publishSet(hv, 'did:plc:c', { tracks: [hit] });

		const charts = await query<Charts>(hv, 'getCharts');

		expect(charts.tracks).toEqual([
			{ id: 'name:ferrule\nhit', title: 'Hit', artist: 'Ferrule', artists: [{ name: 'Ferrule' }], supports: 3 }
		]);
		expect(charts.artists).toEqual([{ name: 'Ferrule', supports: 3, djCount: 3 }]);
	});

	it('gives an artist at most five supports from one DJ', async () => {
		const hv = new FakeHappyView(ROOT);
		await acts(hv, 'a', 'b', 'c');
		const tracks = Array.from({ length: 7 }, (_, i) => ({ title: `Cut ${i}`, artist: 'Kestrel Unit' }));

		for (const dj of ['a', 'b', 'c']) await publishSet(hv, `did:plc:${dj}`, { tracks });

		const charts = await query<Charts>(hv, 'getCharts', { limit: 2 });
		expect(charts.artists).toEqual([{ name: 'Kestrel Unit', supports: 15, djCount: 3 }]);
		expect(charts.tracks.map((track) => track.supports)).toEqual([3, 3]);
	});

	it('narrows to a city and a time, and lists the cities', async () => {
		const hv = new FakeHappyView(ROOT);
		await acts(hv, 'a', 'b', 'c');

		for (const dj of ['a', 'b', 'c']) {
			const did = `did:plc:${dj}`;
			const location = address('Berlin');
			await publishSet(hv, did, { date: '2026-09-10', location, tracks: [{ title: 'September', artist: 'X' }] });
			await publishSet(hv, did, { date: '2026-08-31', location, tracks: [{ title: 'August', artist: 'X' }] });

			await publishSet(hv, did, {
				date: '2026-09-11',
				location: address('Hamburg'),
				tracks: [{ title: 'Harbour', artist: 'Y' }]
			});
		}

		const titles = async (params: Params) =>
			(await query<Charts>(hv, 'getCharts', params)).tracks.map((track) => track.title).sort();

		expect(await titles({})).toEqual(['August', 'Harbour', 'September']);
		expect(await titles({ city: 'berlin', country: 'de' })).toEqual(['August', 'September']);
		expect(await titles({ since: '2026-09-01', before: '2026-10-01' })).toEqual(['Harbour', 'September']);
		expect(await titles({ since: '2026-09-01', city: 'Hamburg', country: 'DE' })).toEqual(['Harbour']);
		expect(await titles({ before: '2026-09-01' })).toEqual(['August']);

		expect((await query<Charts>(hv, 'getCharts', { city: 'Hamburg', country: 'DE' })).places).toEqual([
			{ city: 'Berlin', country: 'DE', count: 6 },
			{ city: 'Hamburg', country: 'DE', count: 3 }
		]);
	});

	it('counts a track as one whether a set names it by MBID (either form), ISRC, or title', async () => {
		const hv = new FakeHappyView(ROOT);
		await acts(hv, 'a', 'b', 'c', 'd');

		const plays = {
			a: {
				title: 'Glow',
				artist: 'Two Rooms',
				artists: [{ artistName: 'Two Rooms', artistMbId: `mbid:${mbid(9)}` }],
				recordingMbId: `mbid:${mbid(1).toUpperCase()}`,
				isrc: 'deaa12600001'
			},
			b: {
				title: 'Glow',
				artist: 'Two Rooms',
				artists: [{ artistName: 'Two Rooms', artistMbId: mbid(9) }],
				recordingMbId: mbid(1)
			},
			// Without a recording MBID, its ISRC finds the recording, and its artist's name finds theirs.
			c: { title: 'Glow (Edit)', artist: 'Two Rooms', isrc: 'DEAA12600001' },
			// With neither, its credit and title find it, in any case and spacing.
			d: { title: 'glow ', artist: 'two  ROOMS' }
		};

		for (const [dj, track] of Object.entries(plays)) await publishSet(hv, `did:plc:${dj}`, { tracks: [track] });

		const charts = await query<Charts>(hv, 'getCharts');
		expect(charts.tracks).toHaveLength(1);
		expect(charts.tracks[0]).toMatchObject({ id: `mbid:${mbid(1)}`, recordingMbId: mbid(1), supports: 4 });
		expect(charts.artists).toEqual([{ name: 'Two Rooms', mbid: mbid(9), supports: 4, djCount: 4 }]);
	});
});

describe('who counts', () => {
	const hit = { title: 'Hit', artist: 'Ferrule' };

	const supports = async (hv: FakeHappyView) =>
		(await query<Charts>(hv, 'getCharts')).tracks.map((track) => track.supports);

	it('counts a crew whose managers each publish the gig as one DJ', async () => {
		const hv = new FakeHappyView(ROOT);
		await publish(hv, {}, { managers: ['did:plc:a', 'did:plc:b'] });
		await acts(hv, 'c', 'd');

		for (const dj of ['a', 'b', 'c']) await publishSet(hv, `did:plc:${dj}`, { tracks: [hit] });

		expect(await supports(hv)).toEqual([]);
		await publishSet(hv, 'did:plc:d', { tracks: [hit] });
		expect(await supports(hv)).toEqual([3]);
		expect((await query<Artist>(hv, 'getArtist', { name: 'Ferrule' })).artist).toEqual({
			name: 'Ferrule',
			supports: 3,
			djCount: 3
		});
	});

	it("stops counting a DJ's sets once they don't manage an active project, and doesn't show them", async () => {
		const hv = new FakeHappyView(ROOT);
		await acts(hv, 'a', 'b');
		await publish(hv, {}, { managers: ['did:plc:c'] }, { rkey: 'crew' });
		await publish(hv, {}, { managers: ['did:plc:d'] }, { rkey: 'later' });

		for (const dj of ['a', 'b', 'c', 'd']) {
			await publishSet(hv, `did:plc:${dj}`, { location: address('Berlin'), tracks: [hit] });
		}

		const plays = async () =>
			(await query<Artist>(hv, 'getArtist', { name: 'Ferrule' })).tracks.flatMap((track) =>
				track.plays.map((play) => play.did)
			);

		expect(await supports(hv)).toEqual([4]);
		await publish(hv, {}, { managers: ['did:plc:c'], status: 'hidden' }, { rkey: 'crew' });
		await publish(hv, {}, { managers: [] }, { rkey: 'later' });

		expect(await supports(hv)).toEqual([]);
		expect((await plays()).sort()).toEqual(['did:plc:a', 'did:plc:b']);
		expect((await query<Artist>(hv, 'getArtist', { name: 'Ferrule' })).djs.map((dj) => dj.did).sort()).toEqual([
			'did:plc:a',
			'did:plc:b'
		]);
		expect((await query<Charts>(hv, 'getCharts')).places).toEqual([{ city: 'Berlin', country: 'DE', count: 2 }]);
	});
});

describe('what counts', () => {
	it("keeps only sets played on a real day, and doesn't credit placeholders", async () => {
		const hv = new FakeHappyView(ROOT);
		await acts(hv, 'a');
		const loose = await publishSet(hv, 'did:plc:a', { date: '2026-9-1' });

		await publishSet(hv, 'did:plc:a', {
			tracks: [
				{ title: 'ID', artist: 'Ferrule' },
				{ title: 'Night Bus', artist: 'ID - ID' },
				{ title: 'Signal', artist: 'Unknown Artist' },
				{
					title: 'Compiled',
					artist: 'Various Artists',
					artists: [
						{ artistName: 'Various Artists', artistMbId: '89ad4ac3-39f7-470e-963a-56509c546377' },
						{ artistName: 'Someone', artistMbId: '125ec42a-7229-4250-afc5-e057484327fe' },
						{ artistName: 'Two Rooms' }
					]
				}
			]
		});

		expect(hv.logs).toEqual([`skipped ${loose.uri}: bad date or tracks`]);
		const { artists } = await query<{ artists: { name: string }[] }>(hv, 'searchArtists');
		expect(artists.map((artist) => artist.name)).toEqual(['Two Rooms']);
	});

	it('tells same-titled tracks apart by their artists when sets give no credit line', async () => {
		const hv = new FakeHappyView(ROOT);
		await acts(hv, 'a', 'b', 'c');

		for (const dj of ['a', 'b', 'c']) {
			await publishSet(hv, `did:plc:${dj}`, {
				tracks: [
					{ title: 'Intro', artists: [{ artistName: 'Ferrule' }] },
					{ title: 'Intro', artists: [{ artistName: 'Two Rooms' }] }
				]
			});
		}

		const charts = await query<Charts>(hv, 'getCharts');
		expect(charts.tracks.map((track) => [track.title, track.artists, track.supports])).toEqual([
			['Intro', [{ name: 'Ferrule' }], 3],
			['Intro', [{ name: 'Two Rooms' }], 3]
		]);
	});

	it('gives a play with only an ISRC the recording MBID its title has, when that ISRC is the title’s only one', async () => {
		const hv = new FakeHappyView(ROOT);
		await acts(hv, 'a', 'b', 'c');
		await publishSet(hv, 'did:plc:a', { tracks: [{ title: 'Glow', artist: 'Two Rooms', recordingMbId: mbid(1) }] });
		await publishSet(hv, 'did:plc:b', { tracks: [{ title: 'Glow', artist: 'Two Rooms', isrc: 'NLA112600087' }] });
		await publishSet(hv, 'did:plc:c', { tracks: [{ title: 'Glow', artist: 'Two Rooms' }] });

		expect((await query<Charts>(hv, 'getCharts')).tracks).toMatchObject([{ id: `mbid:${mbid(1)}`, supports: 3 }]);
	});
});

describe('getArtist and searchArtists', () => {
	const signal = (startSeconds?: number) => ({
		title: 'Signal Path',
		artist: 'Mira Okonkwo',
		artists: [{ artistName: 'Mira Okonkwo', artistMbId: mbid(7) }],
		startSeconds
	});

	const nightBus = {
		title: 'Night Bus',
		artist: 'Ferrule & Mira Okonkwo',
		artists: [{ artistName: 'Ferrule' }, { artistName: 'mira okonkwo' }]
	};

	async function network() {
		const hv = new FakeHappyView(ROOT);
		await publish(hv, { name: 'Neon Tide' }, { slug: 'neon-tide', managers: ['did:plc:a'] });
		await publish(hv, { name: 'Hidden' }, { slug: 'hidden', managers: ['did:plc:a'], status: 'hidden' });
		await publish(hv, { name: 'Halflight' }, { slug: 'halflight', managers: ['did:plc:b'] });

		const late = await publishSet(hv, 'did:plc:a', {
			date: '2026-09-26',
			kind: 'club',
			venue: 'Halle Nord',
			location: address('Berlin'),
			tracks: [nightBus, signal(300)]
		});

		const early = await publishSet(hv, 'did:plc:b', { date: '2026-09-12', tracks: [signal()] });
		const first = await publishSet(hv, 'did:plc:a', { date: '2026-09-01', tracks: [signal(), nightBus] });

		return { hv, late, early, first };
	}

	it("lists an artist's tracks by supports, with every play and the projects of the DJs who played them", async () => {
		const { hv, late, early, first } = await network();
		const result = await query<Artist>(hv, 'getArtist', { mbid: mbid(7) });

		expect(result.artist).toEqual({ name: 'Mira Okonkwo', mbid: mbid(7), supports: 3, djCount: 2 });
		expect(result.tracks.map(({ track, plays }) => [track.title, track.supports, plays])).toEqual([
			[
				'Signal Path',
				2,
				[
					{
						did: 'did:plc:a',
						startSeconds: 300,
						set: {
							...late,
							date: '2026-09-26',
							kind: 'club',
							venue: 'Halle Nord',
							location: address('Berlin'),
							trackCount: 2
						}
					},
					{ did: 'did:plc:b', set: { ...early, date: '2026-09-12', trackCount: 1 } },
					{ did: 'did:plc:a', set: { ...first, date: '2026-09-01', trackCount: 2 } }
				]
			],
			['Night Bus', 1, [expect.objectContaining({ did: 'did:plc:a' }), expect.objectContaining({ did: 'did:plc:a' })]]
		]);
		expect(result.tracks[1].track.artists).toEqual([{ name: 'Ferrule' }, { name: 'mira okonkwo' }]);
		expect(result.djs).toEqual([
			{ did: 'did:plc:a', projects: [{ slug: 'neon-tide', name: 'Neon Tide' }] },
			{ did: 'did:plc:b', projects: [{ slug: 'halflight', name: 'Halflight' }] }
		]);
	});

	it('finds an artist by either form of their MBID, or by a name sets give that MBID, and keeps the two apart', async () => {
		const { hv } = await network();
		const byMbid = await query<Artist>(hv, 'getArtist', { mbid: mbid(7) });

		expect(await query<Artist>(hv, 'getArtist', { mbid: `mbid:${mbid(7).toUpperCase()}` })).toEqual(byMbid);
		expect(await query<Artist>(hv, 'getArtist', { name: ' MIRA  okonkwo' })).toEqual(byMbid);
		const none = { tracks: [], djs: [] };
		expect(await query<Artist>(hv, 'getArtist', { name: 'Nobody' })).toEqual(none);
		expect(await query<Artist>(hv, 'getArtist', { mbid: mbid(8) })).toEqual(none);
		// A name that looks like an MBID is still a name, and a name isn't an MBID.
		expect(await query<Artist>(hv, 'getArtist', { name: mbid(7) })).toEqual(none);
		expect(await query<Artist>(hv, 'getArtist', { mbid: 'Mira Okonkwo' })).toEqual(none);
	});

	it('finds artists by part of their name in any case, most supported first', async () => {
		const { hv } = await network();
		const search = async (params: Params) => (await query<{ artists: unknown[] }>(hv, 'searchArtists', params)).artists;

		expect(await search({ q: 'RULE' })).toEqual([{ name: 'Ferrule', supports: 1, djCount: 1 }]);
		expect(await search({})).toEqual([
			{ name: 'Mira Okonkwo', mbid: mbid(7), supports: 3, djCount: 2 },
			{ name: 'Ferrule', supports: 1, djCount: 1 }
		]);
		expect(await search({ q: 'nobody' })).toEqual([]);
	});
});

describe('searchProjects', () => {
	it('searches by text, distance, genre family, filters, and sorts', async () => {
		const hv = new FakeHappyView(ROOT);
		await publish(hv, {
			name: 'Deep Current',
			genres: ['deep house'],
			fee: { tier: 'emerging' },
			formats: ['vinyl'],
			setTypes: ['warmup']
		});
		await publish(hv, {
			name: 'Iron Pulse',
			genres: ['hard techno'],
			location: { city: 'Hamburg', country: 'DE', ...HAMBURG }
		});
		await publish(hv, { name: 'No Coords', genres: ['house'], location: { city: 'Somewhere', country: 'US' } });
		const search = (params: Params) => query<Page>(hv, 'searchProjects', params);

		expect(names(await search({ q: 'current' }))).toEqual(['Deep Current']);
		expect((await search({ q: 'zzz' })).total).toBe(0);
		expect(names(await search({ genres: 'house' })).sort()).toEqual(['Deep Current', 'No Coords']);
		const near = await search({ latE6: 52_520_000, lngE6: 13_400_000, radiusKm: 50 });
		expect(names(near)).toEqual(['Deep Current']);
		expect(near.projects[0].distanceM).toBeLessThan(1000);
		const wide = await search({ latE6: 52_520_000, lngE6: 13_400_000, radiusKm: 400, sort: 'distance' });
		expect(names(wide)).toEqual(['Deep Current', 'Iron Pulse']);
		expect((await search({ feeTiers: 'emerging' })).total).toBe(1);
		expect((await search({ formats: 'vinyl', setTypes: ['warmup'] })).total).toBe(1);
		expect((await search({ country: 'us' })).total).toBe(1);
		expect((await search({ unclaimed: true })).total).toBe(3);
		expect(names(await search({ sort: 'name', limit: 2, cursor: '2' }))).toEqual(['No Coords']);
		expect((await query<{ genres: { count: number }[] }>(hv, 'getStats')).genres[0]).toMatchObject({ count: 1 });
		expect(await query(hv, 'getStats')).toMatchObject({ total: 3, active: 3, unclaimed: 3 });
	});

	it('shows active projects unless asked for a status or inactive ones, and filters by kind and manager', async () => {
		const hv = new FakeHappyView(ROOT);
		await publish(hv, { name: 'Shown', kind: 'duo' }, { managers: ['did:plc:dj'] });
		await publish(hv, { name: 'Hidden' }, { status: 'hidden', managers: ['did:plc:dj'] });
		await publish(hv, { name: 'Archived' }, { status: 'archived' });
		const search = async (params: Params) => names(await query<Page>(hv, 'searchProjects', params));

		expect(await search({})).toEqual(['Shown']);
		expect(await search({ includeInactive: true })).toEqual(['Archived', 'Hidden', 'Shown']);
		expect(await search({ status: 'hidden' })).toEqual(['Hidden']);
		expect(await search({ includeInactive: true, kinds: ['duo', 'group'] })).toEqual(['Shown']);
		expect(await search({ includeInactive: true, manager: 'did:plc:dj', sort: 'name' })).toEqual(['Hidden', 'Shown']);
		expect(await search({ includeInactive: true, unclaimed: true })).toEqual(['Archived']);
		// HappyView doesn't enforce a list's maxLength, so the script reads only the first 10 kinds.
		const kinds = [...Array.from({ length: 10 }, (_, i) => `kind${i}`), 'duo'];
		expect(await search({ kinds })).toEqual([]);
		expect(await search({ kinds: kinds.slice(1) })).toEqual(['Shown']);
	});

	it('matches every word as a word prefix and ranks name over other fields', async () => {
		const hv = new FakeHappyView(ROOT);
		await publish(hv, { name: 'Pulsewidth', tagline: 'Late night techno' });
		await publish(hv, { name: 'Night Shift', bio: 'Pulse driven sets' });
		await publish(hv, { name: 'Impulse', genres: ['minimal'] });
		await publish(hv, { name: 'Elsewhere', location: { city: 'Leipzig', region: 'Saxony', country: 'DE' } });
		const search = async (q: string) => names(await query<Page>(hv, 'searchProjects', { q }));

		expect(await search('puls')).toEqual(['Pulsewidth', 'Night Shift']);
		// Pulsewidth: night in the tagline (4) and pulse in the name (10). Night Shift: 10 and 1 (bio).
		expect(await search('NIGHT pulse')).toEqual(['Pulsewidth', 'Night Shift']);
		expect(await search('pulse ambient')).toEqual([]);
		expect(await search('minimal')).toEqual(['Impulse']);
		expect(await search('saxo')).toEqual(['Elsewhere']);
		expect(await search('...')).toEqual(['Elsewhere', 'Impulse', 'Night Shift', 'Pulsewidth']);
	});

	it('sorts newest first by default and pages with a cursor', async () => {
		const hv = new FakeHappyView(ROOT);

		for (const name of ['One', 'Two', 'Three']) await publish(hv, { name });
		const first = await query<Page>(hv, 'searchProjects', { limit: 2 });
		expect(first).toMatchObject({ total: 3, cursor: '2' });
		expect(names(first)).toEqual(['Three', 'Two']);
		const last = await query<Page>(hv, 'searchProjects', { limit: 2, cursor: first.cursor ?? '' });
		expect(last.cursor).toBeUndefined();
		expect(names(last)).toEqual(['One']);
		expect(names(await query<Page>(hv, 'searchProjects', { limit: 99, cursor: 'nonsense' }))).toEqual([
			'Three',
			'Two',
			'One'
		]);
	});
});

describe('matchProjects', () => {
	/** The candidate the scoring tests start from. */
	const candidate: JsonObject = {
		genres: ['techno'],
		location: { city: 'Berlin', country: 'DE', ...BERLIN },
		travel: { radiusKm: 50 },
		fee: { tier: 'emerging' },
		capacity: { min: 100, max: 800 },
		setTypes: ['peak'],
		formats: ['vinyl']
	};

	const unlocated = { location: { city: 'Berlin', country: 'DE' } };

	/** Scores one project against a show: the only project in a fresh directory, counted even out of range. */
	async function score(params: Params, profile: JsonObject = {}): Promise<Match> {
		const hv = new FakeHappyView(ROOT);
		await publish(hv, { ...candidate, ...profile });

		return (await query<{ matches: Match[] }>(hv, 'matchProjects', { includeOutOfRange: true, ...params })).matches[0];
	}

	it('gives a perfect local match a perfect score', async () => {
		const result = await score({
			...BERLIN,
			radiusKm: 30,
			genres: 'techno',
			budgetTier: 'emerging',
			capacity: 300,
			setType: 'peak',
			formats: 'vinyl'
		});

		expect(result.score).toBe(100);
		expect(result.inRange).toBe(true);
		expect(result.reasons).toContainEqual({ code: 'plays', genres: ['techno'] });
		expect(result.reasons).toContainEqual({ code: 'local' });
		expect(result.warnings).toEqual([]);
	});

	it('is neutral on criteria that are not specified', async () => {
		const result = await score({ radiusKm: 50 }, unlocated);
		expect(result.score).toBe(100);
		expect(result.project.distanceM).toBeUndefined();
	});

	it('decays with distance and marks far projects out of range unless they travel', async () => {
		const near = await score({ ...BERLIN, radiusKm: 50 }, { location: { city: 'Potsdam', country: 'DE', ...POTSDAM } });
		expect(near.project.distanceM).toBeGreaterThan(20_000);
		expect(near.breakdown.distance).toBeLessThan(100);
		expect(near.inRange).toBe(true);

		const far = await score(
			{ ...BERLIN, radiusKm: 50 },
			{ location: { city: 'Munich', country: 'DE', ...MUNICH }, travel: { radiusKm: 100 } }
		);

		expect(far.inRange).toBe(false);
		expect(far.breakdown.distance).toBe(0);

		const traveller = await score(
			{ ...BERLIN, radiusKm: 50 },
			{ location: { city: 'Munich', country: 'DE', ...MUNICH }, travel: { radiusKm: 1000 } }
		);

		expect(traveller.inRange).toBe(true);
		expect(traveller.breakdown.distance).toBe(50);
		expect(traveller.reasons[0].code).toBe('travels');

		const unknown = await score({ ...BERLIN, radiusKm: 50 }, unlocated);
		expect(unknown.breakdown.distance).toBe(30);
		expect(unknown.warnings).toContainEqual({ code: 'locationUnknown' });
	});

	it('scores budget tiers', async () => {
		const budget = async (feeTier?: string, budgetTier = 'established') =>
			(await score({ radiusKm: 10, budgetTier }, { fee: feeTier ? { tier: feeTier } : undefined })).breakdown.budget;

		expect(await budget('established')).toBe(100);
		expect(await budget('emerging')).toBe(90);
		expect(await budget('headliner')).toBe(35);
		expect(await budget()).toBe(60);
		expect(await budget('headliner', 'community')).toBe(0);
	});

	it('scores capacity, set type, and format fit', async () => {
		expect((await score({ capacity: 1200 })).breakdown.capacity).toBe(50);
		expect((await score({ capacity: 5000 })).breakdown.capacity).toBe(20);
		expect((await score({ capacity: 20 })).warnings).toContainEqual({ code: 'smallerRoom' });
		expect((await score({ capacity: 100 }, { capacity: undefined })).breakdown.capacity).toBe(60);
		expect((await score({ setType: 'closing' })).breakdown.setType).toBe(50);
		expect((await score({ setType: 'closing' }, { setTypes: [] })).breakdown.setType).toBe(60);
		expect((await score({ formats: 'cdj' })).breakdown.format).toBe(40);
		expect((await score({ formats: 'cdj' }, { formats: [] })).breakdown.format).toBe(60);
	});

	it('warns when genres do not overlap, which costs the genre weight', async () => {
		const result = await score({ radiusKm: 10, genres: 'reggaeton' });
		expect(result.breakdown.genre).toBe(0);
		expect(result.score).toBe(60);
		expect(result.warnings).toContainEqual({ code: 'noGenreOverlap' });
	});

	it('explains a mixed match with notes', async () => {
		const result = await score(
			{
				...BERLIN,
				radiusKm: 50,
				genres: ['house', 'techno'],
				budgetTier: 'community',
				capacity: 5000,
				setType: 'peak',
				formats: ['cdj', 'vinyl']
			},
			{ location: { city: 'Potsdam', country: 'DE', ...POTSDAM }, fee: { tier: 'headliner' } }
		);

		expect(result.reasons).toEqual([
			{ code: 'plays', genres: ['techno'] },
			// What the TypeScript haversine made of Berlin to Potsdam: 27.19121258777598 km.
			{ code: 'nearby', distanceM: 27_191 },
			{ code: 'setType', setType: 'peak' },
			{ code: 'formats', formats: ['vinyl'] }
		]);
		expect(result.warnings).toEqual([{ code: 'wellAboveBudget' }, { code: 'biggerRoom' }]);
		expect((await score({ genres: 'house' })).reasons).toEqual([{ code: 'adjacentSound' }]);
		expect((await score({ budgetTier: 'community' })).warnings).toEqual([{ code: 'aboveBudget' }]);
		expect((await score({ capacity: 300, setType: 'warmup' })).reasons).toEqual([{ code: 'fitsRoom' }]);
		expect((await score({ budgetTier: 'headliner' })).reasons).toEqual([{ code: 'fitsBudget' }]);
		expect((await score({ formats: 'cdj' })).warnings).toEqual([{ code: 'differentSetup' }]);
	});

	it.each([
		['exact genres, any case', 'techno', 'Techno', 100],
		["a family's headline genre and a subgenre", 'house', 'deep house', 75],
		['two subgenres of one family', 'deep house', 'tech house', 60],
		['neighbouring families', 'house', 'disco', 30],
		['unrelated families', 'techno', 'reggaeton', 0],
		['an unknown genre', 'vaporwave', 'techno', 0],
		['an unknown genre and itself', 'vaporwave', 'vaporwave', 100],
		['a family by id', 'bass', 'dubstep', 75],
		['an alias', 'garage', '2-step', 75]
	])('scores %s', async (_case, wanted, offered, genre) => {
		expect((await score({ genres: wanted }, { genres: [offered] })).breakdown.genre).toBe(genre);
	});

	it('averages the best match per wanted genre and reports strong matches', async () => {
		const result = await score({ genres: ['techno', 'house'] }, { genres: ['hard techno', 'disco'] });
		// (0.75 + 0.3) / 2
		expect(result.breakdown.genre).toBe(53);
		expect(result.reasons).toEqual([{ code: 'plays', genres: ['hard techno'] }]);
		expect((await score({ genres: 'house' }, { genres: [] })).breakdown.genre).toBe(0);
	});

	it('ranks active projects, drops out-of-range ones, and limits results', async () => {
		const hv = new FakeHappyView(ROOT);
		await publish(hv, { ...candidate, name: 'Disco', genres: ['disco'] });
		await publish(hv, { ...candidate, name: 'Techno' });
		await publish(hv, {
			...candidate,
			name: 'Far',
			location: { city: 'Munich', country: 'DE', ...MUNICH },
			travel: undefined
		});
		await publish(hv, { ...candidate, name: 'Hidden' }, { status: 'hidden' });
		const show = { ...BERLIN, radiusKm: 40, genres: 'techno' };

		const match = async (params: Params) =>
			(await query<{ matches: Match[] }>(hv, 'matchProjects', params)).matches.map(
				(result) => result.project.record.profile.name
			);

		expect(await match(show)).toEqual(['Techno', 'Disco']);
		// Far still plays techno (75), which beats Disco's nearby but unrelated sound (60).
		expect(await match({ ...show, includeOutOfRange: true })).toEqual(['Techno', 'Far', 'Disco']);
		expect(await match({ radiusKm: 40, limit: 1 })).toHaveLength(1);
	});
});

type ShowPage = {
	shows: (Omit<DirectoryShow, 'record' | 'project'> & { record: ShowRecord; project: View })[];
	cursor?: string;
};

const HOUR = 3_600_000;

/** A time `hours` from now, the way a show carries it. */
const fromNow = (hours: number) => new Date(Date.now() + hours * HOUR).toISOString();

let announced = 0;

/**
 * Delivers a show record like Jetstream would, for `project` by `did`, with `fields` over valid defaults, each a day
 * later than the last. Returns its URI and CID.
 */
async function publishShow(
	hv: FakeHappyView,
	did: string,
	project: string,
	fields: JsonObject = {}
): Promise<StrongRef> {
	announced += 1;
	const rkey = `3w${String(announced).padStart(11, '0')}`;

	const value = {
		$type: COLLECTION.show,
		project,
		event: { uri: `at://${did}/community.lexicon.calendar.event/${rkey}`, cid: 'bafyreievent' },
		name: `Show ${announced}`,
		startsAt: fromNow(24 * announced),
		location: { city: 'Berlin', country: 'DE', ...BERLIN },
		createdAt: '2026-10-01T00:00:00.000Z',
		...fields
	};

	const cid = (await cidForLex(jsonToLex(value))).toString();
	await hv.ingest({ did, collection: COLLECTION.show, rkey, cid, record: value });

	return { uri: `at://${did}/${COLLECTION.show}/${rkey}`, cid };
}

let followed = 0;

/** Delivers a follow record like Jetstream would. Returns its URI. */
async function publishFollow(hv: FakeHappyView, did: string, subject: string, fields: JsonObject = {}) {
	followed += 1;
	const rkey = `3f${String(followed).padStart(11, '0')}`;
	const value = { $type: COLLECTION.follow, subject, createdAt: '2026-10-01T00:00:00.000Z', ...fields };
	await hv.ingest({
		did,
		collection: COLLECTION.follow,
		rkey,
		cid: (await cidForLex(jsonToLex(value))).toString(),
		record: value
	});

	return `at://${did}/${COLLECTION.follow}/${rkey}`;
}

const showNames = (page: ShowPage) => page.shows.map((show) => show.record.name);

describe('show record script', () => {
	it('keeps shows by managers of the project they name, with only what listings read', async () => {
		// The queries check managers again, so a probe shows what got stored at all.
		const bundle = happyViewBundle(ROOT);

		const hv = new FakeHappyView(ROOT, {
			lexicons: [
				...bundle.lexicons,
				parseLexiconDoc({ lexicon: 1, id: 'test.fake.stored', defs: { main: { type: 'query' } } })
			],
			scripts: [
				...bundle.scripts,
				{
					id: 'xrpc.query:test.fake.stored',
					body: `function handle() return db.raw("SELECT uri FROM happyview_records WHERE collection = '${COLLECTION.show}'") end`
				}
			]
		});

		const project = await publish(hv, { name: 'Neon Tide' }, { managers: ['did:plc:dj'] });
		// A project in someone else's repo doesn't make its managers count.
		const fake = await publish(hv, {}, { managers: ['did:plc:faker'] }, { did: 'did:plc:stranger' });
		const startsAt = fromNow(48);

		const kept = await publishShow(hv, 'did:plc:dj', project, {
			name: 'Tidal Hours',
			startsAt,
			venue: 'Halle Nord',
			location: { city: 'Berlin', country: 'DE', ...BERLIN, street: 'Nordufer 1' },
			status: 'community.lexicon.calendar.event#scheduled',
			tickets: 'https://tickets.example/tidal',
			description: 'Not copied'
		});

		await publishShow(hv, 'did:plc:faker', fake);
		await publishShow(hv, 'did:plc:dj', fake);
		await publishShow(hv, 'did:plc:nobody', project);
		const broken = await publishShow(hv, 'did:plc:dj', project, { event: 'at://did:plc:dj/x/y' });
		const placeless = await publishShow(hv, 'did:plc:dj', project, { location: { city: 'Berlin' } });
		const unmapped = await publishShow(hv, 'did:plc:dj', project, { location: { city: 'Berlin', country: 'DE' } });
		const scripted = await publishShow(hv, 'did:plc:dj', project, { tickets: 'JavaScript:alert(1)' });

		const { shows } = await query<ShowPage>(hv, 'listShows', { limit: 1 });

		expect(shows).toEqual([
			{
				...kept,
				record: {
					project,
					event: { uri: expect.stringMatching(/^at:\/\/did:plc:dj\//), cid: 'bafyreievent' },
					name: 'Tidal Hours',
					startsAt,
					venue: 'Halle Nord',
					location: { city: 'Berlin', country: 'DE', ...BERLIN },
					status: 'community.lexicon.calendar.event#scheduled',
					tickets: 'https://tickets.example/tidal',
					createdAt: '2026-10-01T00:00:00.000Z'
				},
				project: expect.objectContaining({ uri: project })
			}
		]);
		expect(hv.logs).toEqual([
			`skipped ${broken.uri}: bad event`,
			`skipped ${placeless.uri}: bad place`,
			`skipped ${unmapped.uri}: bad place`
		]);
		expect(await (await hv.fetch('http://happyview.test/xrpc/test.fake.stored')).json()).toEqual([
			{ uri: kept.uri },
			{ uri: scripted.uri }
		]);
		// Only web links stay: listings make the ticket link clickable.
		const [unsafe] = (await query<ShowPage>(hv, 'getShows', { uris: scripted.uri })).shows;
		expect(unsafe.record).not.toHaveProperty('tickets');
	});

	it('removes a deleted show', async () => {
		const hv = new FakeHappyView(ROOT);
		const project = await publish(hv, {}, { managers: ['did:plc:dj'] });
		const show = await publishShow(hv, 'did:plc:dj', project);
		const rkey = show.uri.split('/').at(-1)!;

		expect((await query<ShowPage>(hv, 'getShows', { uris: [show.uri] })).shows).toHaveLength(1);
		await hv.ingest({ did: 'did:plc:dj', collection: COLLECTION.show, rkey, cid: '', record: null });
		expect((await query<ShowPage>(hv, 'getShows', { uris: [show.uri] })).shows).toEqual([]);
	});
});

describe('follow record script and getFollowStatus', () => {
	it("counts each account following one of the root's projects once, and finds an account's follows", async () => {
		const hv = new FakeHappyView(ROOT);
		const project = await publish(hv);
		const other = await publish(hv);
		const fake = await publish(hv, {}, {}, { did: 'did:plc:stranger' });

		const a = await publishFollow(hv, 'did:plc:a', project);
		const first = await publishFollow(hv, 'did:plc:b', project);
		const second = await publishFollow(hv, 'did:plc:b', project);
		await publishFollow(hv, 'did:plc:c', other);
		await publishFollow(hv, 'did:plc:c', fake);
		await publishFollow(hv, 'did:plc:d', project, { createdAt: 20261001 });

		expect(await query(hv, 'getFollowStatus', { project })).toEqual({ followers: 2, follows: [] });
		expect(await query(hv, 'getFollowStatus', { project, actor: 'did:plc:b' })).toEqual({
			followers: 2,
			follows: [first, second]
		});
		expect(await query(hv, 'getFollowStatus', { project, actor: 'did:plc:c' })).toEqual({ followers: 2, follows: [] });
		expect(await query(hv, 'getFollowStatus', { project: fake, actor: 'did:plc:c' })).toEqual({
			followers: 0,
			follows: []
		});

		for (const [did, uri] of [
			['did:plc:a', a],
			['did:plc:b', first]
		])
			await hv.ingest({ did, collection: COLLECTION.follow, rkey: uri.split('/').at(-1)!, cid: '', record: null });
		expect(await query(hv, 'getFollowStatus', { project, actor: 'did:plc:b' })).toEqual({
			followers: 1,
			follows: [second]
		});
	});
});

describe('listShows and getShows', () => {
	it('lists upcoming shows soonest first, only while their announcer manages the active project', async () => {
		const hv = new FakeHappyView(ROOT);
		const project = await publish(hv, {}, { managers: ['did:plc:dj', 'did:plc:co'] }, { rkey: 'tide' });
		const hidden = await publish(hv, {}, { managers: ['did:plc:dj'], status: 'hidden' });
		const other = await publish(hv, {}, { managers: ['did:plc:dj'] });

		const past = await publishShow(hv, 'did:plc:dj', project, { name: 'Past', startsAt: fromNow(-20) });
		await publishShow(hv, 'did:plc:dj', project, { name: 'Tonight', startsAt: fromNow(-6) });
		await publishShow(hv, 'did:plc:dj', project, { name: 'Weekender', startsAt: fromNow(-48), endsAt: fromNow(1) });
		await publishShow(hv, 'did:plc:dj', project, { name: 'Over', startsAt: fromNow(-48), endsAt: fromNow(-1) });
		await publishShow(hv, 'did:plc:co', project, { name: 'Later', startsAt: fromNow(72) });
		// Offsets sort by the instants they mean, not by their text: Sydney's 05:00 is Paris's 20:00 the day before.
		await publishShow(hv, 'did:plc:dj', project, { name: 'Paris', startsAt: '2099-01-01T00:00:00+01:00' });
		await publishShow(hv, 'did:plc:dj', project, { name: 'Sydney', startsAt: '2099-01-01T05:00:00+10:00' });
		await publishShow(hv, 'did:plc:dj', hidden, { name: 'Hidden' });
		await publishShow(hv, 'did:plc:dj', other, { name: 'Elsewhere', startsAt: fromNow(24) });

		const list = async (params: Params = {}) => showNames(await query<ShowPage>(hv, 'listShows', params));

		expect(await list()).toEqual(['Weekender', 'Tonight', 'Elsewhere', 'Later', 'Sydney', 'Paris']);
		expect(await list({ project })).toEqual(['Weekender', 'Tonight', 'Later', 'Sydney', 'Paris']);
		expect(await list({ limit: 2 })).toEqual(['Weekender', 'Tonight']);
		expect((await query<ShowPage>(hv, 'listShows', { limit: 2 })).cursor).toBe('2');
		expect((await query<ShowPage>(hv, 'listShows', { limit: 2, cursor: '4' })).cursor).toBeUndefined();
		expect(await list({ limit: 2, cursor: '2' })).toEqual(['Elsewhere', 'Later']);
		// getShows finds past shows too.
		expect(showNames(await query<ShowPage>(hv, 'getShows', { uris: past.uri }))).toEqual(['Past']);

		await publish(hv, {}, { managers: ['did:plc:dj'] }, { rkey: 'tide' });
		expect(await list({ project })).toEqual(['Weekender', 'Tonight', 'Sydney', 'Paris']);
	});

	it('finds shows near a place, and nearest first among the projects someone follows', async () => {
		const hv = new FakeHappyView(ROOT);
		const tide = await publish(hv, {}, { managers: ['did:plc:dj'] });
		const other = await publish(hv, {}, { managers: ['did:plc:dj'] });

		const at = (city: string, point: { latE6: number; lngE6: number }) => ({
			location: { city, country: 'DE', ...point }
		});

		await publishShow(hv, 'did:plc:dj', tide, { name: 'Berlin', ...at('Berlin', BERLIN) });
		await publishShow(hv, 'did:plc:dj', tide, { name: 'Hamburg', ...at('Hamburg', HAMBURG) });
		await publishShow(hv, 'did:plc:dj', other, { name: 'Potsdam', ...at('Potsdam', POTSDAM) });
		await publishShow(hv, 'did:plc:dj', tide, { name: 'Munich', ...at('Munich', MUNICH) });
		// Inside the box SQL narrows by first, but 56 km from Berlin.
		await publishShow(hv, 'did:plc:dj', other, {
			name: 'Corner',
			...at('Corner', { latE6: 52_870_000, lngE6: 14_000_000 })
		});
		await publishFollow(hv, 'did:plc:fan', tide);

		const near = await query<ShowPage>(hv, 'listShows', { ...BERLIN, radiusKm: 50 });
		expect(showNames(near)).toEqual(['Berlin', 'Potsdam']);
		expect(near.shows[1].distanceM).toBeGreaterThan(20_000);
		expect(near.shows[1].distanceM).toBeLessThan(30_000);

		const following = { follower: 'did:plc:fan', ...HAMBURG, sort: 'distance' };
		expect(showNames(await query<ShowPage>(hv, 'listShows', following))).toEqual(['Hamburg', 'Berlin', 'Munich']);
		expect(showNames(await query<ShowPage>(hv, 'listShows', { follower: 'did:plc:fan' }))).toEqual([
			'Berlin',
			'Hamburg',
			'Munich'
		]);
		expect(await query(hv, 'listShows', { follower: 'did:plc:nobody' })).toEqual({ shows: [] });
	});

	it('finds shows and projects across the antimeridian', async () => {
		const hv = new FakeHappyView(ROOT);
		// Taveuni, Fiji, straddles 180°: Waiyevo is east of it, Vuna west, 20 km apart.
		const waiyevo = { latE6: -16_790_000, lngE6: 179_980_000 };
		const vuna = { latE6: -16_950_000, lngE6: -179_900_000 };
		const location = { city: 'Vuna', country: 'FJ', ...vuna };
		const project = await publish(hv, { name: 'Island Time', location }, { managers: ['did:plc:dj'] });
		await publishShow(hv, 'did:plc:dj', project, { name: 'Beach', location });
		await publishShow(hv, 'did:plc:dj', project, { name: 'Harbour', location: { ...location, ...waiyevo } });

		// From either side of 180°, the box reaches round to the other.
		for (const from of [waiyevo, vuna])
			expect(showNames(await query<ShowPage>(hv, 'listShows', { ...from, radiusKm: 50 }))).toEqual([
				'Beach',
				'Harbour'
			]);
		expect(names(await query<Page>(hv, 'searchProjects', { ...waiyevo, radiusKm: 50 }))).toEqual(['Island Time']);
		expect(names(await query<Page>(hv, 'searchProjects', { ...vuna, radiusKm: 5 }))).toEqual(['Island Time']);
		expect(
			names(await query<Page>(hv, 'searchProjects', { latE6: -16_950_000, lngE6: 179_000_000, radiusKm: 50 }))
		).toEqual([]);
	});
});

describe('instruction budget', () => {
	it('searches and matches 1,000 projects well under the limit', { tags: ['budget'] }, async () => {
		const hv = new FakeHappyView(ROOT);
		const tiers = ['community', 'emerging', 'established', 'headliner'];

		// Stored without running the record script 1,000 times: it keeps valid records as they are, and only the
		// queries over them are measured.
		const records = Array.from({ length: 1000 }, (_unused, i) =>
			projectRecord({
				name: `Project ${i}`,
				tagline: 'Deep grooves all night long',
				bio: 'A collective that plays warehouse parties and small clubs. '.repeat(5),
				highlights: ['Berghain', 'Tresor', 'Panorama Bar'],
				genres: Array.from({ length: 1 + (i % 6) }, (_, j) => KNOWN_GENRES[(i * 7 + j * 13) % KNOWN_GENRES.length]),
				location: {
					city: `City ${i % 50}`,
					country: 'DE',
					latE6: 47_000_000 + ((i * 7919) % 8_000_000),
					lngE6: 6_000_000 + ((i * 104_729) % 9_000_000)
				},
				travel: { radiusKm: i % 500 },
				fee: { tier: tiers[i % 4] },
				capacity: { min: 100, max: 800 + i },
				formats: i % 2 ? ['vinyl'] : ['cdj', 'vinyl'],
				setTypes: ['peak', 'closing']
			})
		);

		// One real CID for them all: the queries return it, and nothing here tells records apart by it.
		const cid = (await cidForLex(jsonToLex(records[0].value))).toString();

		hv.seed(records.map(({ did, rkey, value }) => ({ did, collection: COLLECTION.project, rkey, cid, record: value })));

		const show = { ...BERLIN, budgetTier: 'emerging', capacity: 300, setType: 'peak', formats: 'vinyl' };

		const queries: [keyof typeof DIRECTORY, Params][] = [
			['searchProjects', {}],
			['searchProjects', { q: 'deep grooves' }],
			['searchProjects', { ...BERLIN, radiusKm: 2000, genres: ['techno', 'house'] }],
			['searchProjects', { ...BERLIN, radiusKm: 2000, sort: 'distance' }],
			['matchProjects', { ...show, genres: ['techno', 'house', 'disco'], includeOutOfRange: true }],
			['matchProjects', { ...show, genres: KNOWN_GENRES.slice(0, 12), includeOutOfRange: true }]
		];

		const used: Record<string, number> = {};

		for (const [name, params] of queries) {
			await query(hv, name, params);
			used[`${name} ${JSON.stringify(params)}`] = hv.instructions;
		}

		expect(Object.entries(used).filter(([, instructions]) => instructions >= 750_000)).toEqual([]);
	});

	it('lists from 1,000 upcoming shows well under the limit', { tags: ['budget'] }, async () => {
		const hv = new FakeHappyView(ROOT);

		const projects = Array.from({ length: 50 }, (_unused, i) =>
			projectRecord({ bio: 'A collective that plays warehouse parties. '.repeat(10) }, { managers: [`did:plc:dj${i}`] })
		);

		const shows = Array.from({ length: 1000 }, (_unused, i) => {
			const project = projects[i % 50];

			return {
				did: `did:plc:dj${i % 50}`,
				collection: COLLECTION.show,
				rkey: `3w${String(i).padStart(11, '0')}`,
				record: {
					project: `at://${ROOT}/${COLLECTION.project}/${project.rkey}`,
					event: { uri: `at://did:plc:dj${i % 50}/community.lexicon.calendar.event/${i}`, cid: 'bafyreievent' },
					name: `Show ${i}`,
					startsAt: fromNow(1 + i),
					location: {
						city: `City ${i % 50}`,
						country: 'DE',
						latE6: 47_000_000 + ((i * 7919) % 8_000_000),
						lngE6: 6_000_000 + ((i * 104_729) % 9_000_000)
					},
					createdAt: '2026-10-01T00:00:00.000Z'
				}
			};
		});

		const follows = projects.slice(0, 25).map(({ rkey }, i) => ({
			did: 'did:plc:fan',
			collection: COLLECTION.follow,
			rkey: `3f${String(i).padStart(11, '0')}`,
			record: { subject: `at://${ROOT}/${COLLECTION.project}/${rkey}`, createdAt: '2026-10-01T00:00:00.000Z' }
		}));

		const cid = (await cidForLex(jsonToLex(projects[0].value))).toString();
		hv.seed([
			...projects.map(({ did, rkey, value }) => ({ did, collection: COLLECTION.project, rkey, cid, record: value })),
			...[...shows, ...follows].map((record) => ({ ...record, cid }))
		]);

		const queries: Params[] = [
			{ limit: 100 },
			{ ...BERLIN, radiusKm: 2000, limit: 100 },
			{ ...BERLIN, radiusKm: 2000, limit: 100, cursor: '900' },
			{ follower: 'did:plc:fan', ...BERLIN, sort: 'distance', limit: 100 }
		];

		const used: Record<string, number> = {};

		for (const params of queries) {
			expect((await query<ShowPage>(hv, 'listShows', params)).shows.length).toBeGreaterThan(0);
			used[JSON.stringify(params)] = hv.instructions;
		}

		expect(Object.entries(used).filter(([, instructions]) => instructions >= 750_000)).toEqual([]);
	});

	it(
		'keeps a 500-track set and counts supports over 2,000 sets and 500-track ones well under the limit',
		{ tags: ['budget'], timeout: 120_000 },
		async () => {
			const hv = new FakeHappyView(ROOT);
			const djs = Array.from({ length: 50 }, (_, i) => `did:plc:dj${i}`);
			await publish(hv, { name: 'Crew' }, { slug: 'crew', managers: [djs[0]] });

			await publishSet(hv, djs[0], {
				tracks: Array.from({ length: 500 }, (_, i) => ({
					title: `Track ${i}`,
					artist: `Artist ${i} & Guest`,
					artists: [{ artistName: `Artist ${i}`, artistMbId: mbid(i) }, { artistName: 'Guest' }],
					recordingMbId: mbid(i),
					isrc: `DEAA126${String(i).padStart(5, '0')}`,
					startSeconds: i * 300
				}))
			});
			const used: Record<string, number> = {};
			used['record script, 500 tracks'] = hv.instructions;

			// Stored as the record script leaves them, without running it 2,000 times (each tracklist as its named tracks):
			// 50 DJs, each with a project of their own, with 40 sets of 25 tracks, from 3,000 tracks by 600 artists, some
			// played far more than others, and a third of them without IDs. One of them also has 20 sets of 500 tracks, all
			// crediting one artist.
			let seed = 1;

			const random = () => {
				seed = (seed * 48_271) % 2_147_483_647;

				return seed / 2_147_483_647;
			};

			const stored = (n: number) => {
				const artist = `Artist ${n % 600}`;
				const ids = n % 3 ? { recordingMbId: mbid(n), isrc: `DEAA126${String(n).padStart(5, '0')}` } : {};

				const artists = [
					{ artistName: artist, key: artist.toLowerCase(), ...(n % 3 && { artistMbId: mbid(100_000 + (n % 600)) }) }
				];

				return { title: `Track ${n}`, artist, artists, ...ids, key: `${artist.toLowerCase()}\ntrack ${n}` };
			};

			const cid = (await cidForLex(jsonToLex({ seeded: true }))).toString();
			const cities = ['Berlin', 'Hamburg', 'Leipzig', 'Cologne', 'Munich'];

			const projects = djs.slice(1).map((did) => projectRecord({}, { managers: [did] }));

			hv.seed(
				projects.map(({ did, rkey, value }) => ({ did, collection: COLLECTION.project, rkey, cid, record: value }))
			);

			const marathon = Array.from({ length: 500 }, (_track, t) => ({
				title: `Long ${t}`,
				artist: 'Marathon',
				artists: [{ artistName: 'Marathon', key: 'marathon' }],
				key: `marathon\nlong ${t}`,
				startSeconds: t * 60
			}));

			const long = Array.from({ length: 20 }, (_set, s) => ({
				did: djs[1],
				collection: COLLECTION.djSet,
				rkey: `3u${String(s).padStart(11, '0')}`,
				cid,
				record: {
					date: `2026-08-${String(s + 1).padStart(2, '0')}`,
					kind: 'mix',
					trackCount: 500,
					tracks: marathon,
					namedTracks: marathon
				}
			}));

			hv.seed(long);

			hv.seed(
				djs.flatMap((did, d) =>
					Array.from({ length: 40 }, (_set, s) => {
						const tracks = Array.from({ length: 25 }, (_track, t) => ({
							...stored(Math.floor(3000 * random() ** 3)),
							startSeconds: t * 240
						}));

						return {
							did,
							collection: COLLECTION.djSet,
							rkey: `3t${String(d * 40 + s).padStart(11, '0')}`,
							cid,
							record: {
								date: new Date(Date.UTC(2026, 0, 1 + ((d * 7 + s * 11) % 280))).toISOString().slice(0, 10),
								kind: 'club',
								location: { country: 'DE', locality: cities[(d + s) % cities.length] },
								trackCount: tracks.length,
								tracks,
								namedTracks: tracks
							}
						};
					})
				)
			);

			const queries: [keyof typeof DIRECTORY, Params][] = [
				['getCharts', {}],
				['getCharts', { since: '2026-09-01', before: '2026-10-01', city: 'Berlin', country: 'DE', limit: 100 }],
				['getArtist', { name: 'Artist 0' }],
				['getArtist', { mbid: mbid(100_001) }],
				['getArtist', { name: 'Marathon' }],
				['listSets', { actors: djs[1], limit: 100 }],
				['listRecords', { actor: djs[1], collection: COLLECTION.djSet, limit: 100 }],
				['searchArtists', { q: 'artist 1', limit: 50 }],
				['searchArtists', {}]
			];

			for (const [name, params] of queries) {
				await query(hv, name, params);
				used[`${name} ${JSON.stringify(params)}`] = hv.instructions;
			}

			expect(Object.entries(used).filter(([, instructions]) => instructions >= 750_000)).toEqual([]);
		}
	);
});
