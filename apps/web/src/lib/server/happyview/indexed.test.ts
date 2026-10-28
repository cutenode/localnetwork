import { cidForLex } from '@atproto/lex-cbor';
import { jsonToLex } from '@atproto/lex-json';
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import { COLLECTION, DIRECTORY } from '#lib/nsid.ts';
import { silentLogger } from '../logger';
import { FakeHappyView } from '../testing/happyview';
import { HappyViewClient } from './client';
import { IndexedRepo } from './indexed';

const DJ = 'did:plc:dj';

/** A HappyView with an API key over `hv`, recording each query and backfill it gets. */
function admin(hv: FakeHappyView) {
	const calls: string[] = [];

	const client = new HappyViewClient({
		url: 'http://happyview.test',
		apiKey: 'hv_test',
		fetch: async (input, init) => {
			const request = new Request(input, init);
			const { pathname } = new URL(request.url);

			if (pathname !== '/admin/backfill') {
				calls.push(pathname);

				return hv.fetch(request);
			}

			const { collection, did } = z.object({ collection: z.string(), did: z.string() }).parse(await request.json());
			calls.push(`backfill ${collection} ${did}`);

			return Response.json({ id: 'job', status: 'running' });
		}
	});

	return { client, calls };
}

/** Delivers a DJ's set like Jetstream would. Returns its URI and CID. */
async function publishSet(hv: FakeHappyView, rkey: string) {
	const record = { $type: COLLECTION.djSet, date: '2026-10-01', tracks: [], createdAt: '2026-10-02T00:00:00.000Z' };
	const cid = (await cidForLex(jsonToLex(record))).toString();
	await hv.ingest({ did: DJ, collection: COLLECTION.djSet, rkey, cid, record });

	return { uri: `at://${DJ}/${COLLECTION.djSet}/${rkey}`, cid };
}

describe('IndexedRepo', () => {
	it('looks up any number of records, 25 at a time, each once', async () => {
		const hv = new FakeHappyView('did:plc:root');
		const { client, calls } = admin(hv);
		const repo = new IndexedRepo(client, silentLogger);
		const sets = await Promise.all(Array.from({ length: 30 }, (_, i) => publishSet(hv, `3set${i}`)));
		const uris = sets.map((set) => set.uri);

		const found = await repo.getRecords([...uris, uris[0], `at://${DJ}/${COLLECTION.djSet}/missing`]);

		expect([...found.keys()].toSorted()).toEqual(uris.toSorted());
		expect(found.get(sets[0].uri)?.cid).toBe(sets[0].cid);
		expect(calls).toEqual([`/xrpc/${DIRECTORY.getRecords}`, `/xrpc/${DIRECTORY.getRecords}`]);
		expect(await repo.getRecord(DJ, COLLECTION.djSet, 'missing')).toBeNull();
	});

	it("lists all of an account's records a page at a time, or up to a number of pages", async () => {
		const hv = new FakeHappyView('did:plc:root');
		const { client, calls } = admin(hv);
		const repo = new IndexedRepo(client, silentLogger);

		const sets = await Promise.all(
			Array.from({ length: 150 }, (_, i) => publishSet(hv, `3set${String(i).padStart(3, '0')}`))
		);

		const all = await repo.listAll(DJ, COLLECTION.djSet);
		expect(all.map((record) => record.uri)).toEqual(sets.map((set) => set.uri).toReversed());
		expect(calls).toEqual([`/xrpc/${DIRECTORY.listRecords}`, `/xrpc/${DIRECTORY.listRecords}`]);
		expect(await repo.listAll(DJ, COLLECTION.djSet, { pages: 1 })).toHaveLength(100);
	});

	it("waits for a write HappyView serves, and has it re-read the account's repo for one it doesn't", async () => {
		const hv = new FakeHappyView('did:plc:root');
		const { client, calls } = admin(hv);
		const repo = new IndexedRepo(client, silentLogger, 0);
		const set = await publishSet(hv, '3set');

		await repo.awaitWrite(DJ, COLLECTION.djSet, '3set', set.cid);
		await repo.awaitWrite(DJ, COLLECTION.djSet, 'gone', null);
		expect(calls.filter((call) => call.startsWith('backfill'))).toEqual([]);

		await repo.awaitWrite(DJ, COLLECTION.djSet, '3set', null);
		await repo.awaitWrite(DJ, COLLECTION.djSet, '3set', 'bafynewer');
		expect(calls.filter((call) => call.startsWith('backfill'))).toEqual([
			`backfill ${COLLECTION.djSet} ${DJ}`,
			`backfill ${COLLECTION.djSet} ${DJ}`
		]);
	});
});
