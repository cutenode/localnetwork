import { describe, expect, it } from 'vitest';
import { COLLECTION, DIRECTORY, NS } from '#lib/nsid.ts';
import { FakeHappyView } from '../testing/happyview';
import { happyViewBundle } from './bundle';

describe('happyViewBundle', () => {
	it('ships the project record, its defs, and the directory queries, with a script for each', () => {
		const { lexicons, scripts } = happyViewBundle('did:plc:root');

		expect(lexicons.map((doc) => doc.id).sort()).toEqual(
			[`${NS}.defs`, COLLECTION.project, `${NS}.directory.defs`, ...Object.values(DIRECTORY)].sort()
		);
		expect(scripts.map((script) => script.id).sort()).toEqual(
			[`record.index:${COLLECTION.project}`, ...Object.values(DIRECTORY).map((nsid) => `xrpc.query:${nsid}`)].sort()
		);
		// The record gate carries nothing but the root DID, so no shared code can break it.
		expect(scripts.find((script) => script.id.startsWith('record.'))?.body).not.toMatch(/TAXONOMY|local function list/);
		expect(scripts.find((script) => script.id.startsWith('xrpc.'))?.body).toMatch(/TAXONOMY[^]*local function list/);
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
