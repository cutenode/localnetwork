import { setTimeout as sleep } from 'node:timers/promises';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { z } from 'zod';
import { COLLECTION } from '#lib/nsid.ts';
import { createTestServices } from '../testing/core';
import { HappyViewClient } from './client';
import { refreshForScript } from './refresh';

describe('re-reading a collection for a new record script', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices();
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const managers = Array.from({ length: 10 }, (_, i) => `did:plc:manager${i}`);

	/** A HappyView that takes a moment over each backfill, and turns away the ones for `refuse`. */
	function happyView(refuse = new Set<string>()) {
		const backfilled: string[] = [];
		let running = 0;
		let most = 0;
		let polls = 0;

		const client = new HappyViewClient({
			url: 'http://happyview.test',
			apiKey: 'hv_test',
			fetch: async (input, init) => {
				const request = new Request(input, init);
				const { pathname } = new URL(request.url);

				if (pathname === '/admin/backfill/status') polls += 1;

				if (pathname !== '/admin/backfill') return Response.json([]);
				const { collection, did } = z.object({ collection: z.string(), did: z.string() }).parse(await request.json());
				running += 1;
				most = Math.max(most, running);
				await sleep(5);
				running -= 1;
				backfilled.push(`${collection} ${did}`);

				return refuse.has(did) ? new Response('no', { status: 500 }) : Response.json({ id: did, status: 'running' });
			}
		});

		return { client, backfilled, most: () => most, polls: () => polls };
	}

	it("re-reads every account's records once per script version, four at a time, without waiting for them", async () => {
		const hv = happyView();

		const refresh = (script: string, uploaded: boolean) =>
			refreshForScript(t.core, hv.client, {
				collection: COLLECTION.djSet,
				script,
				uploaded,
				accounts: async () => managers
			});

		await refresh('v1', true);
		expect(hv.backfilled.toSorted()).toEqual(managers.map((did) => `${COLLECTION.djSet} ${did}`).toSorted());
		expect(hv.most()).toBe(4);
		expect(hv.polls()).toBe(0);

		await refresh('v1', false);
		expect(hv.backfilled).toHaveLength(10);
		await refresh('v2', false);
		expect(hv.backfilled).toHaveLength(20);
		// HappyView was given the same script again (a new instance, say), so it needs the records again.
		await refresh('v2', true);
		expect(hv.backfilled).toHaveLength(30);
	});

	it('keeps each collection to its own script version', async () => {
		const hv = happyView();

		const refresh = (collection: string) =>
			refreshForScript(t.core, hv.client, {
				collection,
				script: 'v1',
				uploaded: false,
				accounts: async () => managers
			});

		await refresh(COLLECTION.djSet);
		await refresh(COLLECTION.epk);
		await refresh(COLLECTION.djSet);
		expect(hv.backfilled.filter((entry) => entry.startsWith(`${COLLECTION.epk} `))).toHaveLength(10);
		expect(hv.backfilled).toHaveLength(20);
	});

	it('tries again until every re-read has started', async () => {
		const refuse = new Set([managers[3]]);
		const hv = happyView(refuse);

		const refresh = () =>
			refreshForScript(t.core, hv.client, {
				collection: COLLECTION.djSet,
				script: 'v1',
				uploaded: false,
				accounts: async () => managers
			});

		await refresh();
		await refresh();
		expect(hv.backfilled).toHaveLength(20);
		refuse.clear();
		await refresh();
		await refresh();
		expect(hv.backfilled).toHaveLength(30);
	});
});
