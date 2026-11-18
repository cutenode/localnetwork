import { describe, expect, it } from 'vitest';
import { USERINPUT } from '#lib/bug-report.ts';
import { BSKY_FOLLOW, BSKY_PROFILE, CALENDAR_EVENT, COLLECTION, DIRECTORY, NS, VERIFICATION } from '#lib/nsid.ts';
import { FakeHappyView } from '../testing/happyview';
import { chunk, happyViewBundle } from './bundle';

describe('happyViewBundle', () => {
	it('ships the records and queries lua/ has a script for, and the defs they use', () => {
		const { lexicons, network, scripts } = happyViewBundle('did:plc:root');

		const records = [
			COLLECTION.project,
			COLLECTION.djProfile,
			COLLECTION.djSet,
			COLLECTION.djRecording,
			COLLECTION.epk,
			COLLECTION.show,
			COLLECTION.follow,
			COLLECTION.profile,
			COLLECTION.listingProfile,
			COLLECTION.listing,
			`${NS}.locallibrary.settings`,
			CALENDAR_EVENT,
			VERIFICATION,
			BSKY_PROFILE,
			BSKY_FOLLOW,
			USERINPUT.space
		];

		const others = new Set<string>([CALENDAR_EVENT, VERIFICATION, BSKY_PROFILE, BSKY_FOLLOW, USERINPUT.space]);

		// The other authorities' lexicons are HappyView's to track from the network, ingesting those a script handles.
		expect(network.toSorted((a, b) => a.nsid.localeCompare(b.nsid))).toEqual(
			[
				'com.atproto.repo.strongRef',
				'com.atproto.label.defs',
				...['address', 'fsq', 'geo', 'hthree'].map((name) => `community.lexicon.location.${name}`),
				...others
			]
				.toSorted()
				.map((nsid) => ({ nsid, indexed: others.has(nsid) }))
		);
		expect(lexicons.map((doc) => doc.id).sort()).toEqual(
			[
				`${NS}.defs`,
				`${NS}.section.defs`,
				`${NS}.people.defs`,
				`${NS}.dj.defs`,
				`${NS}.epk.defs`,
				`${NS}.directory.defs`,
				...records.filter((nsid) => !others.has(nsid)),
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
			expect(gate.body).not.toMatch(/TAXONOMY|local function atUris/);
		expect(scripts.find((script) => script.id.startsWith('xrpc.'))?.body).toMatch(/TAXONOMY[^]*local function atUris/);
	});

	it('starts each script with the modules its files require, once each', () => {
		const { scripts } = happyViewBundle('did:plc:root');
		const body = (nsid: string) => scripts.find((script) => script.id.endsWith(`:${nsid}`))?.body ?? '';

		const query = body(DIRECTORY.getRecords);
		expect(query).toMatch(
			/^local json = require\('internal.json'\)\nlocal sql = require\('happyview.sql'\)\nlocal ROOT_DID/
		);
		expect(query.match(/require\(/g)).toHaveLength(2);
		// A gate requires the SQL library inside its pcall, so a missing one is a skip rather than a stored record.
		const gate = body(COLLECTION.show);
		expect(gate).toMatch(
			/^local json = require\('internal.json'\)\nlocal log = require\('internal.logging'\)\nlocal ROOT_DID/
		);
		expect(gate).toContain("\tlocal sql = require('happyview.sql')");

		expect(chunk(["local a = require('x')\nlocal b = 1", 'local a = require("x")\nlocal c = 2'])).toBe(
			"local a = require('x')\nlocal b = 1\nlocal c = 2"
		);
		expect(() => chunk(["local a = require('x')\n", "local a = require('y')\n"])).toThrow(
			'Two Lua files bind a to different modules'
		);
	});

	it("puts each lexicon after the ones it refers to, since HappyView stores them in the bundle's order", () => {
		const { lexicons, network } = happyViewBundle('did:plc:root');
		// The sync has HappyView track the network lexicons before it uploads ours.
		const tracked = network.map(({ nsid }) => nsid);

		for (const [index, doc] of lexicons.entries()) {
			const refs = [...JSON.stringify(doc).matchAll(/"((?:com|community|computer)\.[\w.]+?)(?:#\w+)?"/g)].map(
				([, nsid]) => nsid
			);

			for (const ref of refs.filter((nsid) => nsid !== doc.id))
				expect([...tracked, ...lexicons.slice(0, index).map((earlier) => earlier.id)]).toContain(ref);
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
