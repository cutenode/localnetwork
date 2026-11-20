import { cidForLex } from '@atproto/lex-cbor';
import { jsonToLex } from '@atproto/lex-json';
import { beforeAll, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { JsonRecord } from '../../src/lib/server/atproto/repo-client.ts';
import { happyViewBundle } from '../../src/lib/server/happyview/bundle.ts';
import { HappyViewClient } from '../../src/lib/server/happyview/client.ts';
import { FakeHappyView, type RecordEvent } from '../../src/lib/server/testing/happyview.ts';
import { compare, type ParityCase, parityCases, syncLikeTheApp } from './happyview-parity.ts';

const ROOT = 'did:plc:root';

const DJ = 'did:plc:dj';

const bundle = happyViewBundle(ROOT);

const collections = bundle.scripts.flatMap(({ id }) => (id.startsWith('record.index:') ? [id.split(':')[1]] : []));

const createdAt = '2026-01-01T00:00:00.000Z';

async function event(did: string, collection: string, rkey: string, record: JsonRecord): Promise<RecordEvent> {
	return { did, collection, rkey, cid: (await cidForLex(jsonToLex(record))).toString(), record };
}

/** The root's project, which the DJ manages, and the DJ's stats: what each test's old instance holds. */
const records = await Promise.all([
	event(ROOT, COLLECTION.project, '3kproject0001', {
		slug: 'night-shift',
		profile: {
			name: 'Night Shift',
			kind: 'solo',
			genres: ['techno'],
			location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 }
		},
		managers: [DJ],
		status: 'active',
		revision: 1,
		createdBy: ROOT,
		createdAt,
		updatedAt: createdAt
	}),
	event(DJ, COLLECTION.djProfile, 'self', {
		generatedAt: createdAt,
		tempo: { low: 120, high: 132, median: 126 },
		genres: [{ name: 'techno', share: 40 }]
	})
]);

/** A HappyView that has already indexed `records`. */
async function indexed(): Promise<FakeHappyView> {
	const hv = new FakeHappyView(ROOT, bundle);

	for (const record of records) await hv.ingest(record);

	return hv;
}

/**
 * A client for `hv` that also takes the lexicon and script uploads, which FakeHappyView doesn't serve: it reports none
 * stored and accepts each, since it already runs the bundle.
 */
function client(hv: FakeHappyView): HappyViewClient {
	const fetch: typeof globalThis.fetch = async (input, init) => {
		const request = new Request(input, init);
		const path = new URL(request.url).pathname;

		if (!path.startsWith('/admin/lexicons') && path !== '/admin/scripts') return hv.fetch(request);

		if (request.method === 'POST') return Response.json({});

		return path === '/admin/scripts' ? Response.json([]) : Response.json({ error: 'NotFound' }, { status: 404 });
	};

	return new HappyViewClient({ url: 'http://happyview.test', apiKey: 'hv_test', fetch });
}

describe('happyview parity', () => {
	/** The old instance and what to ask it, which no test changes, so they're made once for every test. */
	let old: HappyViewClient;
	let cases: ParityCase[];

	beforeAll(async () => {
		old = client(await indexed());
		cases = await parityCases(old, ROOT, collections);
	});

	it('asks about every project, manager and record the old instance knows, and finds no difference in a copy', async () => {
		expect(cases).toEqual(
			expect.arrayContaining([
				{ name: 'getStats', params: {} },
				{ name: 'getProject', params: { uri: `at://${ROOT}/${COLLECTION.project}/3kproject0001`, counts: true } },
				{ name: 'listRecords', params: { actor: DJ, collection: COLLECTION.djProfile, limit: 100 } },
				{ name: 'getRecords', params: { uris: expect.arrayContaining([`at://${DJ}/${COLLECTION.djProfile}/self`]) } }
			])
		);
		expect(await compare(cases, old, client(await indexed()))).toEqual([]);
	});

	it('reports what a new instance lacks until it syncs the way the app does', async () => {
		const fresh = new FakeHappyView(ROOT, bundle);

		fresh.repo = (did, collection) =>
			records.filter((record) => record.did === did && record.collection === collection);

		const next = client(fresh);

		const before = await compare(cases, old, next);
		expect(before.map(({ name }) => name)).toEqual(expect.arrayContaining(['getStats', 'getProject', 'listRecords']));
		expect(before[0].diff).toContain('Expected values to be strictly deep-equal');

		const statuses = await syncLikeTheApp(next, bundle, { rootDid: ROOT, waitMs: 1_000 });
		expect(Object.keys(statuses)).toEqual(['completed']);
		expect(await compare(cases, old, next)).toEqual([]);
	});

	it('counts a re-read that fails, and a query that fails on one side only, as results rather than stopping', async () => {
		const fresh = new FakeHappyView(ROOT, bundle);

		fresh.repo = (did, collection) => {
			if (did === DJ) throw new Error('PDS unreachable');

			return records.filter((record) => record.did === did && record.collection === collection);
		};

		const statuses = await syncLikeTheApp(client(fresh), bundle, { rootDid: ROOT, waitMs: 1_000 });
		expect(Object.keys(statuses).some((status) => status.startsWith('error: '))).toBe(true);

		const down = new HappyViewClient({ url: 'http://down.test', fetch: async () => new Response('', { status: 502 }) });
		const [mismatch] = await compare([{ name: 'getStats', params: {} }], old, down);
		expect(mismatch.diff).toContain('HTTP 502');
	});
});
