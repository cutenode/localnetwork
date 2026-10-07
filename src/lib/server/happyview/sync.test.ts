import { describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { happyViewBundle } from './bundle';
import { HappyViewClient } from './client';
import { syncHappyView } from './sync';

const ROOT = 'did:plc:root';

const API_KEY = 'hv_test';

/** HappyView's admin API in memory: what it stores, in what order, and what it was asked to backfill. */
class FakeAdmin {
	readonly lexicons = new Map<string, { revision: number; json: string; backfill: boolean }>();
	readonly scripts = new Map<string, string>();
	readonly backfills: { collection: string; did: string }[] = [];
	readonly uploads: string[] = [];
	/** How many status checks a backfill job reports `running` before it finishes. */
	backfillPolls = 0;
	backfillResult = 'completed';
	statusChecks = 0;
	private polls = 0;

	readonly fetch: typeof globalThis.fetch = async (input, init) => {
		const request = new Request(input, init);
		const path = new URL(request.url).pathname;

		if (request.headers.get('authorization') !== `Bearer ${API_KEY}`) return new Response('', { status: 401 });

		if (request.method === 'GET' && path.startsWith('/admin/lexicons/')) {
			const id = path.slice('/admin/lexicons/'.length);
			const stored = this.lexicons.get(id);

			if (!stored) return Response.json({ error: 'NotFound' }, { status: 404 });

			return Response.json({ id, revision: stored.revision, lexicon_json: JSON.parse(stored.json) });
		}

		if (request.method === 'GET' && path === '/admin/scripts') {
			return Response.json([...this.scripts].map(([id, body]) => ({ id, body, script_type: 'lua' })));
		}

		if (request.method === 'GET' && path === '/admin/backfill/status') {
			this.statusChecks++;
			const status = this.polls++ < this.backfillPolls ? 'running' : this.backfillResult;

			return Response.json(this.backfills.map((_, index) => ({ id: `job${index}`, status })));
		}

		const body = await request.json();

		if (path === '/admin/lexicons') {
			const revision = (this.lexicons.get(body.lexicon_json.id)?.revision ?? 0) + 1;

			// serde_json hands objects back with their keys sorted.
			const json = JSON.stringify(body.lexicon_json, (_key, value) =>
				value?.constructor === Object
					? Object.fromEntries(Object.entries(value).sort(([a], [b]) => (a < b ? -1 : 1)))
					: value
			);

			this.lexicons.set(body.lexicon_json.id, { revision, json, backfill: body.backfill });
			this.uploads.push(body.lexicon_json.id);

			return Response.json({ id: body.lexicon_json.id, revision });
		}

		if (path === '/admin/backfill') {
			this.backfills.push(body);
			this.polls = 0;

			return Response.json({ id: `job${this.backfills.length - 1}`, status: 'running' }, { status: 201 });
		}

		if (path !== '/admin/scripts') return new Response('', { status: 404 });
		this.scripts.set(body.id, body.body);
		this.uploads.push(body.id);

		return Response.json({}, { status: 201 });
	};
}

describe('syncHappyView', () => {
	it('uploads only what changed, then backfills the root repo', async () => {
		const admin = new FakeAdmin();
		const client = new HappyViewClient({ url: 'http://happyview.test', apiKey: API_KEY, fetch: admin.fetch });
		const bundle = happyViewBundle(ROOT);

		expect(await syncHappyView(client, bundle, ROOT)).toEqual({
			lexicons: bundle.lexicons.map((doc) => doc.id),
			scripts: bundle.scripts.map((script) => script.id)
		});
		expect(await syncHappyView(client, bundle, ROOT)).toEqual({ lexicons: [], scripts: [] });

		const [script, ...scripts] = bundle.scripts;
		const [lexicon, ...lexicons] = bundle.lexicons;

		const edited = {
			lexicons: [{ ...lexicon, description: 'Edited' }, ...lexicons],
			scripts: [{ ...script, body: `${script.body}\n-- edited` }, ...scripts]
		};

		expect(await syncHappyView(client, edited, ROOT)).toEqual({ lexicons: [lexicon.id], scripts: [script.id] });
		// The record script is in place before the record lexicon starts ingestion.
		const first = admin.uploads.slice(0, bundle.lexicons.length + bundle.scripts.length);
		expect(first.indexOf(`record.index:${COLLECTION.project}`)).toBeLessThan(first.indexOf(COLLECTION.project));
		expect(admin.scripts.get(script.id)).toMatch(/-- edited$/);
		expect([...admin.lexicons].map(([id, stored]) => [id, stored.revision])).toEqual(
			bundle.lexicons.map((doc) => [doc.id, doc.id === lexicon.id ? 2 : 1])
		);
		// A backfill on upload would crawl the whole network; the targeted one covers the root.
		expect([...admin.lexicons.values()].every((stored) => !stored.backfill)).toBe(true);
		expect(admin.backfills).toEqual(Array.from({ length: 3 }, () => ({ collection: COLLECTION.project, did: ROOT })));
	});

	it('waits for the backfill, and fails when it fails', async () => {
		const admin = new FakeAdmin();
		const client = new HappyViewClient({ url: 'http://happyview.test', apiKey: API_KEY, fetch: admin.fetch });
		const bundle = happyViewBundle(ROOT);
		admin.backfillPolls = 2;

		await syncHappyView(client, bundle, ROOT);
		// Two checks see it running, the third sees it done.
		expect(admin.statusChecks).toBe(3);

		admin.backfillResult = 'failed';
		await expect(syncHappyView(client, bundle, ROOT)).rejects.toThrow(`HappyView's backfill of ${ROOT} failed`);
	});

	it('fails without a backfill when HappyView refuses the key', async () => {
		const admin = new FakeAdmin();
		const client = new HappyViewClient({ url: 'http://happyview.test', apiKey: 'hv_wrong', fetch: admin.fetch });

		await expect(syncHappyView(client, happyViewBundle(ROOT), ROOT)).rejects.toMatchObject({ status: 401 });
		expect(admin.backfills).toEqual([]);
	});
});
