import { describe, expect, it } from 'vitest';
import { COLLECTION, DIRECTORY, NS } from '#lib/nsid.ts';
import { FakeHappyView } from '../testing/happyview';
import { happyViewBundle } from './bundle';

describe('happyViewBundle', () => {
	it('ships the project, set, show, and follow records, the defs they use, and the directory queries, with a script for each', () => {
		const { lexicons, scripts } = happyViewBundle('did:plc:root');
		const records = [COLLECTION.project, COLLECTION.djSet, COLLECTION.show, COLLECTION.follow];

		expect(lexicons.map((doc) => doc.id).sort()).toEqual(
			[
				'com.atproto.repo.strongRef',
				'community.lexicon.location.address',
				`${NS}.defs`,
				`${NS}.dj.defs`,
				`${NS}.directory.defs`,
				...records,
				...Object.values(DIRECTORY)
			].sort()
		);
		expect(scripts.map((script) => script.id).sort()).toEqual(
			[
				...records.map((nsid) => `record.index:${nsid}`),
				...Object.values(DIRECTORY).map((nsid) => `xrpc.query:${nsid}`)
			].sort()
		);

		// The record gates carry nothing but the root DID, so no shared code can break them.
		for (const gate of scripts.filter((script) => script.id.startsWith('record.')))
			expect(gate.body).not.toMatch(/TAXONOMY|local function list/);
		expect(scripts.find((script) => script.id.startsWith('xrpc.'))?.body).toMatch(/TAXONOMY[^]*local function list/);
	});

	it("puts each lexicon after the ones it refers to, since HappyView stores them in the bundle's order", () => {
		const { lexicons } = happyViewBundle('did:plc:root');

		for (const [index, doc] of lexicons.entries()) {
			const refs = [...JSON.stringify(doc).matchAll(/"((?:com|community|computer)\.[\w.]+?)(?:#\w+)?"/g)].map(
				([, nsid]) => nsid
			);

			for (const ref of refs.filter((nsid) => nsid !== doc.id))
				expect(lexicons.slice(0, index).map((earlier) => earlier.id)).toContain(ref);
		}
	});

	it('quotes the root DID so nothing in it can end the string early', async () => {
		const root = 'did:web:]]=]';
		const hv = new FakeHappyView(root);

		await hv.ingest({
			did: root,
			collection: COLLECTION.project,
			rkey: 'only',
			cid: 'bafy',
			record: {
				slug: 'only',
				profile: { name: 'Only', kind: 'solo', genres: [], location: { city: 'Berlin', country: 'DE' } },
				managers: [],
				status: 'active',
				revision: 1,
				createdBy: root,
				createdAt: '2026-01-01T00:00:00.000Z',
				updatedAt: '2026-01-01T00:00:00.000Z'
			}
		});

		const response = await hv.fetch(`http://happyview.test/xrpc/${DIRECTORY.getStats}`);
		expect(await response.json()).toMatchObject({ total: 1 });
	});
});
