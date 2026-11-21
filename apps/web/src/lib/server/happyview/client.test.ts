import type { JsonValue } from '@atproto/lex-json';
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import { COLLECTION } from '#lib/nsid.ts';
import { FakeHappyView } from '../testing/happyview';
import { HappyViewClient, HappyViewError } from './client';

const URL_BASE = 'http://happyview.test';

/** A HappyView that answers every request with `body`, and remembers what it was asked. */
function serving(body: JsonValue, status = 200) {
	const requests: Request[] = [];

	const fetch: typeof globalThis.fetch = async (input, init) => {
		requests.push(new Request(input, init));

		return Response.json(body, { status });
	};

	return { requests, fetch };
}

describe('HappyViewClient', () => {
	it('sends params the way XRPC does, with arrays as repeated keys', async () => {
		const happyView = serving({ projects: [], total: 0 });
		const client = new HappyViewClient({ url: URL_BASE, fetch: happyView.fetch });

		await client.query('searchProjects', {
			q: 'deep house',
			latE6: 52_520_000,
			genres: ['deep house', 'techno'],
			kinds: [],
			includeInactive: true,
			country: undefined
		});

		// HappyView percent-decodes without reading `+` as a space.
		expect(happyView.requests[0].url).toBe(
			`${URL_BASE}/xrpc/computer.localnetwork.directory.searchProjects?q=deep%20house&latE6=52520000&genres=deep%20house&genres=techno&includeInactive=true`
		);
	});

	it('refuses params its lexicon does not allow, before asking', async () => {
		const happyView = serving({ projects: [], total: 0 });
		const client = new HappyViewClient({ url: URL_BASE, fetch: happyView.fetch });

		await expect(client.query('searchProjects', { limit: 500 })).rejects.toThrow(/limit/);
		await expect(client.query('getProjects', { uris: ['not a uri'] })).rejects.toThrow(/uris/);
		expect(happyView.requests).toEqual([]);
	});

	it('rejects answers that do not match the lexicon', async () => {
		const client = (body: JsonValue) => new HappyViewClient({ url: URL_BASE, fetch: serving(body).fetch });

		await expect(client({ projects: 'none' }).query('getProjects', { uris: [] })).rejects.toThrow(/projects/);
		await expect(client({ total: 1 }).query('searchProjects')).rejects.toThrow(/projects/);
		await expect(
			client({ project: { uri: 'at://did:plc:root/computer.localnetwork.project/1', cid: 'nope', record: {} } }).query(
				'getProject',
				{ slug: 'x' }
			)
		).rejects.toThrow(/cid/);
	});

	it('reports HTTP failures with their status', async () => {
		const client = new HappyViewClient({
			url: URL_BASE,
			fetch: serving({ error: 'script_error', message: 'boom' }, 500).fetch
		});

		const failure = await client.query('getStats').catch((error: Error) => error);
		expect(failure).toBeInstanceOf(HappyViewError);
		expect(failure).toMatchObject({ status: 500, message: expect.stringContaining('boom') });
	});

	it('reports an unreachable HappyView as unavailable', async () => {
		const client = new HappyViewClient({
			url: URL_BASE,
			fetch: async () => {
				throw new TypeError('fetch failed');
			}
		});

		await expect(client.query('getStats')).rejects.toEqual(
			new HappyViewError(503, 'HappyView /xrpc/computer.localnetwork.directory.getStats: fetch failed')
		);
	});

	it('signs admin calls with the API key and refuses them without one', async () => {
		const happyView = serving([]);
		await new HappyViewClient({ url: URL_BASE, apiKey: 'hv_secret', fetch: happyView.fetch }).scripts();
		expect(happyView.requests[0].headers.get('authorization')).toBe('Bearer hv_secret');

		await expect(new HappyViewClient({ url: URL_BASE, fetch: happyView.fetch }).scripts()).rejects.toThrow(
			/HAPPYVIEW_API_KEY/
		);
		expect(happyView.requests).toHaveLength(1);
	});

	it('names its API client on directory queries, and only there', async () => {
		const happyView = serving([]);
		const client = { key: 'hvc_key', secret: 'hvs_secret' };
		const happyViewClient = new HappyViewClient({ url: URL_BASE, apiKey: 'hv_admin', client, fetch: happyView.fetch });

		await happyViewClient.query('getProjects', { uris: [] }).catch(() => undefined);
		await happyViewClient.scripts();
		await new HappyViewClient({ url: URL_BASE, fetch: happyView.fetch })
			.query('getProjects', { uris: [] })
			.catch(() => undefined);

		const sent = happyView.requests.map((request) => ({
			path: new URL(request.url).pathname,
			key: request.headers.get('x-client-key'),
			secret: request.headers.get('x-client-secret'),
			authorization: request.headers.get('authorization')
		}));

		expect(sent).toEqual([
			{
				path: '/xrpc/computer.localnetwork.directory.getProjects',
				key: 'hvc_key',
				secret: 'hvs_secret',
				authorization: null
			},
			{ path: '/admin/scripts', key: null, secret: null, authorization: 'Bearer hv_admin' },
			{ path: '/xrpc/computer.localnetwork.directory.getProjects', key: null, secret: null, authorization: null }
		]);
	});

	it('says whether HappyView answers', async () => {
		expect(await new HappyViewClient({ url: URL_BASE, fetch: serving({}).fetch }).health()).toBe(true);
		expect(await new HappyViewClient({ url: URL_BASE, fetch: serving({}, 503).fetch }).health()).toBe(false);

		const unreachable = new HappyViewClient({
			url: URL_BASE,
			fetch: () => Promise.reject(new TypeError('fetch failed'))
		});

		expect(await unreachable.health()).toBe(false);
	});

	it('re-reads any number of accounts in jobs of at most 500, and returns every job', async () => {
		const hv = new FakeHappyView('did:plc:root');
		const read: string[] = [];
		const asked: string[][] = [];

		hv.repo = (did) => {
			read.push(did);

			return [];
		};

		const client = new HappyViewClient({
			url: URL_BASE,
			apiKey: 'hv_test',
			fetch: async (input, init) => {
				const request = new Request(input, init);

				if (new URL(request.url).pathname === '/admin/backfill')
					asked.push(z.object({ dids: z.array(z.string()) }).parse(await request.clone().json()).dids);

				return hv.fetch(request);
			}
		});

		const dids = Array.from({ length: 1_001 }, (_, i) => `did:plc:account${i}`);
		const jobs = await client.backfill(COLLECTION.djSet, dids);

		expect(asked.map((chunk) => chunk.length)).toEqual([500, 500, 1]);
		expect(read).toEqual(dids);
		expect(jobs).toHaveLength(3);
		expect(await Promise.all(jobs.map((job) => client.backfillStatus(job)))).toEqual([
			'completed',
			'completed',
			'completed'
		]);
		expect(await client.backfill(COLLECTION.djSet, [])).toEqual([]);
		expect(asked).toHaveLength(3);
	});
});

describe("HappyViewClient's health reads", () => {
	const admin = (body: JsonValue, status = 200) => {
		const happyView = serving(body, status);

		return { ...happyView, client: new HappyViewClient({ url: URL_BASE, apiKey: 'hv_admin', fetch: happyView.fetch }) };
	};

	it('reads whether Jetstream is connected and how far behind it is, as HappyView reports it', async () => {
		const { client, requests } = admin({
			protocol: 'v2',
			host: 'jetstream1.us-east.bsky.network',
			connected: true,
			compression: true,
			applied_seq: 27_293_468_903,
			lag_ms: 956,
			idle_ms: 781,
			worker_queue_depths: [0, 0],
			worker_busy_since_ms: [null, null]
		});

		expect(await client.jetstreamStatus()).toEqual({ connected: true, lagMs: 956 });
		expect(requests[0].url).toBe(`${URL_BASE}/admin/jetstream/status`);
		expect(requests[0].headers.get('authorization')).toBe('Bearer hv_admin');
	});

	it('reads how many dead letters HappyView holds', async () => {
		const { client, requests } = admin({ count: 3 });

		expect(await client.deadLetterCount()).toBe(3);
		expect(requests[0].url).toBe(`${URL_BASE}/admin/dead-letters/count`);
	});

	it('sums every outcome but ok as failures, busiest trigger first', async () => {
		const script = (trigger: string, runs: number, outcomes: Record<string, number>, p95: number) => ({
			trigger,
			kind: 'xrpc_query',
			layer: 'dashboard',
			runs,
			outcomes,
			wall_ms: { max: p95 * 2, mean: p95 / 2, p50: p95 / 3, p95, p99: p95 },
			instructions_mean: 500
		});

		const { client, requests } = admin({
			enabled: true,
			overflow: 0,
			window: '24h',
			scripts: [
				script('xrpc.query:computer.localnetwork.probe.sql', 17, { ok: 17 }, 71.577),
				script('record:computer.localnetwork.project', 40, { ok: 36, trap: 2, error: 1, timeout: 1 }, 12.5)
			]
		});

		expect(await client.scriptMetrics('24h')).toEqual({
			enabled: true,
			scripts: [
				{ trigger: 'record:computer.localnetwork.project', runs: 40, failures: 4, p95Ms: 12.5 },
				{ trigger: 'xrpc.query:computer.localnetwork.probe.sql', runs: 17, failures: 0, p95Ms: 71.577 }
			]
		});
		expect(requests[0].url).toBe(`${URL_BASE}/admin/metrics/scripts?window=24h`);
	});

	it('says when script metrics are off', async () => {
		expect(await admin({ enabled: false, scripts: [], window: '24h' }).client.scriptMetrics('24h')).toEqual({
			enabled: false,
			scripts: []
		});
	});

	it("reports a key without the read's permission as a HappyView error", async () => {
		const { client } = admin({ error: 'forbidden' }, 403);

		const reads = [() => client.jetstreamStatus(), () => client.deadLetterCount(), () => client.scriptMetrics('24h')];

		for (const read of reads) {
			const failure = await read().catch((error: Error) => error);

			expect(failure).toBeInstanceOf(HappyViewError);
			expect(failure).toMatchObject({ status: 403 });
		}
	});
});
