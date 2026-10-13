import { cidForLex } from '@atproto/lex-cbor';
import { type JsonObject, jsonToLex } from '@atproto/lex-json';
import { parseLexiconDoc } from '@atproto/lexicon';
import { describe, expect, it } from 'vitest';
import { COLLECTION, DIRECTORY } from '#lib/nsid.ts';
import { KNOWN_GENRES } from '#lib/taxonomy/genres.ts';
import type { DjSetView, ProjectRecord, StrongRef } from '#lib/types.ts';
import { lexicons } from '../atproto/lexicons';
import { FakeHappyView } from '../testing/happyview';
import { happyViewBundle } from './bundle';
import type { DirectoryMatch, DirectoryOutputs, DirectoryProject } from './client';

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

	const cid = (await cidForLex(jsonToLex(value))).toString();
	await hv.ingest({ did, collection: COLLECTION.project, rkey, cid, record: value });

	return `at://${did}/${COLLECTION.project}/${rkey}`;
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
	it("keeps sets by the root's projects' managers only, without their tracks", async () => {
		const hv = new FakeHappyView(ROOT);
		// A project in someone else's repo doesn't make its managers count.
		await publish(hv, {}, { managers: ['did:plc:faker'] }, { did: 'did:plc:stranger' });
		await publish(hv, {}, { managers: ['did:plc:dj'], status: 'hidden' });

		const kept = await publishSet(hv, 'did:plc:dj', {
			title: 'Closing',
			kind: 'club',
			venue: 'Tresor',
			location: { $type: 'community.lexicon.location.address', country: 'DE', locality: 'Berlin', street: 'Köpenicker' }
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
				location: { country: 'DE', locality: 'Berlin' },
				trackCount: 2
			}
		]);
		expect(hv.logs).toEqual([`skipped ${broken.uri}: bad date or tracks`]);
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

describe('instruction budget', () => {
	it('searches and matches 1,000 projects well under the limit', async () => {
		const hv = new FakeHappyView(ROOT);
		const tiers = ['community', 'emerging', 'established', 'headliner'];

		for (let i = 0; i < 1000; i++) {
			await publish(hv, {
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
			});
		}

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
});
