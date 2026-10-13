import { describe, expect, it } from 'vitest';
import { DEMO_DJ_PROFILE, DEMO_DJ_SETS } from '../../../../scripts/seed-data.ts';
import { PublicRepo } from '../atproto/public-repo';
import { silentLogger } from '../logger';
import { DjService } from './dj';

const DJ = 'did:plc:dj';

const OTHER = 'did:plc:other';

const PDS = 'https://pds.example';

/** A PDS with neon's records on it, answering getRecord and listRecords as a real one does. */
function fakePds(records: Record<string, { profile?: unknown; sets?: unknown[] }>, options: { down?: boolean } = {}) {
	const calls: string[] = [];

	const fetchImpl: typeof fetch = async (input) => {
		const url = new URL(input instanceof Request ? input.url : input);
		calls.push(url.pathname);

		if (options.down) return new Response('busy', { status: 503 });
		const repo = records[url.searchParams.get('repo') ?? ''];

		if (url.pathname.endsWith('getRecord')) {
			return repo?.profile
				? Response.json({ uri: `at://${url.searchParams.get('repo')}/x/self`, value: repo.profile })
				: Response.json({ error: 'RecordNotFound' }, { status: 400 });
		}

		const sets = repo?.sets ?? [];

		return Response.json({
			records: sets.map((value, i) => ({ uri: `at://dj/computer.localnetwork.dj.set/${i}`, cid: 'c', value }))
		});
	};

	return { fetchImpl, calls };
}

const identity = { resolve: async () => ({ pds: PDS }) };

describe('DjService', () => {
	it("reads an account's stats and its sets, newest first", async () => {
		const pds = fakePds({ [DJ]: { profile: DEMO_DJ_PROFILE, sets: DEMO_DJ_SETS.toReversed() } });
		const service = new DjService(new PublicRepo(identity, silentLogger, pds.fetchImpl), silentLogger);

		const [activity] = await service.activity([{ did: DJ, handle: 'neon.test' }]);

		expect(activity.profile?.tempo?.median).toBe(130);
		expect(activity.sets.map((set) => set.date)).toEqual(['2026-09-26', '2026-09-12', '2026-08-30']);
		expect(activity.handle).toBe('neon.test');
	});

	it('leaves out accounts with nothing on file and skips sets that do not parse', async () => {
		const pds = fakePds({ [DJ]: { sets: [DEMO_DJ_SETS[0], { date: 'someday' }] } });
		const service = new DjService(new PublicRepo(identity, silentLogger, pds.fetchImpl), silentLogger);

		const activity = await service.activity([
			{ did: DJ, handle: null },
			{ did: OTHER, handle: null }
		]);

		expect(activity).toHaveLength(1);
		expect(activity[0].profile).toBeNull();
		expect(activity[0].sets).toHaveLength(1);
	});

	it('treats a PDS that is down as nothing on file', async () => {
		const service = new DjService(
			new PublicRepo(identity, silentLogger, fakePds({}, { down: true }).fetchImpl),
			silentLogger
		);

		expect(await service.activity([{ did: DJ, handle: null }])).toEqual([]);
	});

	it('asks the PDS again only after a few minutes', async () => {
		const pds = fakePds({ [DJ]: { profile: DEMO_DJ_PROFILE } });
		let now = 0;
		const service = new DjService(new PublicRepo(identity, silentLogger, pds.fetchImpl), silentLogger, () => now);

		await service.activity([{ did: DJ, handle: null }]);
		now = 60_000;
		await service.activity([{ did: DJ, handle: 'renamed.test' }]);
		expect(pds.calls).toHaveLength(2);

		now = 6 * 60_000;
		await service.activity([{ did: DJ, handle: null }]);
		expect(pds.calls).toHaveLength(4);
	});

	it('hands over records without waiting only while every account is fresh in the cache', async () => {
		const pds = fakePds({ [DJ]: { profile: DEMO_DJ_PROFILE } });
		let now = 0;
		const service = new DjService(identity, silentLogger, pds.fetchImpl, () => now);

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
