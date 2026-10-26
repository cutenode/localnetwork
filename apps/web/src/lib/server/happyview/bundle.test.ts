import { describe, expect, it } from 'vitest';
import { USERINPUT } from '#lib/bug-report.ts';
import { BSKY_FOLLOW, BSKY_PROFILE, CALENDAR_EVENT, COLLECTION, DIRECTORY, NS, VERIFICATION } from '#lib/nsid.ts';
import { FakeHappyView } from '../testing/happyview';
import { happyViewBundle } from './bundle';

describe('happyViewBundle', () => {
	it('ships the records and queries lua/ has a script for, and the defs they use', () => {
		const { lexicons, scripts } = happyViewBundle('did:plc:root');

		const records = [
			COLLECTION.project,
			COLLECTION.djProfile,
			COLLECTION.djSet,
			COLLECTION.djRecording,
			COLLECTION.epk,
			COLLECTION.show,
			COLLECTION.follow,
			`${NS}.locallibrary.settings`,
			CALENDAR_EVENT,
			VERIFICATION,
			BSKY_PROFILE,
			BSKY_FOLLOW,
			USERINPUT.space
		];

		expect(lexicons.map((doc) => doc.id).sort()).toEqual(
			[
				'com.atproto.repo.strongRef',
				'com.atproto.label.defs',
				...['address', 'fsq', 'geo', 'hthree'].map((name) => `community.lexicon.location.${name}`),
				`${NS}.defs`,
				`${NS}.dj.defs`,
				`${NS}.epk.defs`,
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

		// The record gates carry nothing but the root DID and the bug report space, so no shared code can break them.
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

	it('gives the record gates the bug report space, or nil without one', () => {
		const space = `at://did:plc:userinput/${USERINPUT.space}/3kspace`;

		const gate = (bundle: ReturnType<typeof happyViewBundle>) =>
			bundle.scripts.find((script) => script.id === `record.index:${USERINPUT.space}`)?.body;

		expect(gate(happyViewBundle('did:plc:root', space))).toContain(`local USERINPUT_SPACE = [[\n${space}]]`);
		expect(gate(happyViewBundle('did:plc:root'))).toContain('local USERINPUT_SPACE = nil');
		expect(
			happyViewBundle('did:plc:root', space).scripts.find((script) => script.id.startsWith('xrpc.'))?.body
		).not.toContain('USERINPUT_SPACE');
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
