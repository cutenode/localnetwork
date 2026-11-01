import { cidForLex } from '@atproto/lex-cbor';
import { type JsonObject, jsonToLex } from '@atproto/lex-json';
import { describe, expect, it } from 'vitest';
import { COLLECTION, DIRECTORY } from '#lib/nsid.ts';
import { DEMO_DJ_PROFILE, DEMO_DJ_SETS } from '../../../../scripts/seed-data.ts';
import { HappyViewClient } from '../happyview/client';
import { IndexedRepo } from '../happyview/indexed';
import { silentLogger } from '../logger';
import { FakeHappyView } from '../testing/happyview';
import { DjService } from './dj';

const DJ = 'did:plc:dj';

const OTHER = 'did:plc:other';

type DemoSet = (typeof DEMO_DJ_SETS)[number];

/** A HappyView that has `records` of each account, as Jetstream would have brought them, and counts the queries. */
async function happyView(
	records: Record<string, { profile?: typeof DEMO_DJ_PROFILE; sets?: DemoSet[] }>,
	down = false
) {
	const hv = new FakeHappyView('did:plc:root');
	const calls: string[] = [];

	const ingest = async (did: string, collection: string, rkey: string, value: typeof DEMO_DJ_PROFILE | DemoSet) => {
		const record: JsonObject = JSON.parse(JSON.stringify({ $type: collection, ...value }));
		await hv.ingest({ did, collection, rkey, cid: (await cidForLex(jsonToLex(record))).toString(), record });
	};

	for (const [did, { profile, sets = [] }] of Object.entries(records)) {
		if (profile) await ingest(did, COLLECTION.djProfile, 'self', profile);

		for (const [i, set] of sets.entries())
			await ingest(did, COLLECTION.djSet, `3set${String(i).padStart(9, '0')}`, set);
	}

	const directory = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async (input, init) => {
			const request = new Request(input, init);
			calls.push(new URL(request.url).pathname);

			return down ? new Response('busy', { status: 503 }) : hv.fetch(request);
		}
	});

	return { repo: new IndexedRepo(directory, silentLogger), calls };
}

describe('DjService', () => {
	it("reads an account's stats and its sets, newest first, with each set's tracklist", async () => {
		const { repo } = await happyView({ [DJ]: { profile: DEMO_DJ_PROFILE, sets: DEMO_DJ_SETS.toReversed() } });
		const service = new DjService(repo, silentLogger);

		const [activity] = await service.activity([{ did: DJ, handle: 'neon.test' }]);

		expect(activity.profile?.tempo?.median).toBe(130);
		// The demo sets are listed newest first, and played some days before today.
		expect(activity.sets.map((set) => set.date)).toEqual(DEMO_DJ_SETS.map((set) => set.date));
		expect(activity.sets[0].tracks).toHaveLength(DEMO_DJ_SETS[0].tracks.length);
		expect(activity.sets[0].tracks[0]).toMatchObject({ bpmCentis: expect.any(Number), key: expect.any(String) });
		expect(activity.handle).toBe('neon.test');
	});

	it('leaves out accounts with nothing on file and skips sets that do not parse', async () => {
		const { repo } = await happyView({
			[DJ]: { sets: [DEMO_DJ_SETS[0], { ...DEMO_DJ_SETS[1], tracks: [{ bpmCentis: 'fast' }] }] }
		});

		const service = new DjService(repo, silentLogger);

		const activity = await service.activity([
			{ did: DJ, handle: null },
			{ did: OTHER, handle: null }
		]);

		expect(activity).toHaveLength(1);
		expect(activity[0].profile).toBeNull();
		expect(activity[0].sets).toHaveLength(1);
	});

	it('treats a HappyView that is down as nothing on file', async () => {
		const service = new DjService((await happyView({}, true)).repo, silentLogger);

		expect(await service.activity([{ did: DJ, handle: null }])).toEqual([]);
	});

	it("asks for every account's stats in one lookup, and again only after a few minutes", async () => {
		const { repo, calls } = await happyView({ [DJ]: { profile: DEMO_DJ_PROFILE }, [OTHER]: { sets: DEMO_DJ_SETS } });
		let now = 0;
		const service = new DjService(repo, silentLogger, () => now);
		const lookups = () => calls.filter((path) => path === `/xrpc/${DIRECTORY.getRecords}`).length;

		const accounts = [
			{ did: DJ, handle: null },
			{ did: OTHER, handle: null }
		];

		expect((await service.activity(accounts)).map((entry) => entry.did)).toEqual([DJ, OTHER]);
		expect(lookups()).toBe(1);
		expect(calls).toHaveLength(3);

		now = 60_000;
		await service.activity([{ did: DJ, handle: 'renamed.test' }]);
		expect(calls).toHaveLength(3);

		now = 6 * 60_000;
		await service.activity(accounts);
		expect(lookups()).toBe(2);
		expect(calls).toHaveLength(6);
	});

	it('hands over records without waiting only while every account is fresh in the cache', async () => {
		const { repo } = await happyView({ [DJ]: { profile: DEMO_DJ_PROFILE } });
		let now = 0;
		const service = new DjService(repo, silentLogger, () => now);

		const accounts = [
			{ did: DJ, handle: 'neon.test' },
			{ did: OTHER, handle: null }
		];

		expect(service.fresh([])).toEqual([]);
		expect(service.fresh(accounts)).toBeUndefined();

		await service.activity(accounts);
		expect(service.fresh(accounts)?.map((entry) => entry.handle)).toEqual(['neon.test']);
		expect(service.fresh([...accounts, { did: 'did:plc:new', handle: null }])).toBeUndefined();

		now = 6 * 60_000;
		expect(service.fresh(accounts)).toBeUndefined();
	});
});
