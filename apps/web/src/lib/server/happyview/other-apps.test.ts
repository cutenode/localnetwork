import { cidForLex } from '@atproto/lex-cbor';
import { type JsonObject, jsonToLex } from '@atproto/lex-json';
import { describe, expect, it } from 'vitest';
import { USERINPUT } from '#lib/bug-report.ts';
import { BSKY_FOLLOW, BSKY_PROFILE, COLLECTION, VERIFICATION } from '#lib/nsid.ts';
import { FakeHappyView, type RecordEvent } from '../testing/happyview';
import { happyViewBundle } from './bundle';
import { type DirectoryRecord, HappyViewClient } from './client';

// The record gates for the other apps' collections HappyView keeps for the site: Bluesky's verifications, profiles and
// follows, and userinput.app's space.

const ROOT = 'did:plc:root';

const DJ = 'did:plc:dj';

const SPACE = `at://did:plc:userinput/${USERINPUT.space}/3kspace`;

/** A record event as Jetstream would bring it, with `$type` set and its real CID. */
async function event(did: string, collection: string, rkey: string, record: JsonObject): Promise<RecordEvent> {
	const value = { $type: collection, ...record };

	return { did, collection, rkey, cid: (await cidForLex(jsonToLex(value))).toString(), record: value };
}

/** Ingests a record and returns its URI. */
async function ingest(hv: FakeHappyView, did: string, collection: string, rkey: string, record: JsonObject) {
	await hv.ingest(await event(did, collection, rkey, record));

	return `at://${did}/${collection}/${rkey}`;
}

/** What HappyView serves of `uris`, read the way the app reads it. */
async function stored(hv: FakeHappyView, uris: string[]): Promise<DirectoryRecord[]> {
	return (await new HappyViewClient({ url: 'http://happyview.test', fetch: hv.fetch }).query('getRecords', { uris }))
		.records;
}

const values = async (hv: FakeHappyView, uris: string[]) => (await stored(hv, uris)).map((record) => record.value);

/** A root project managed by `managers`. */
const project = (hv: FakeHappyView, rkey: string, managers: string[]) =>
	ingest(hv, ROOT, COLLECTION.project, rkey, {
		slug: rkey,
		profile: { name: rkey, kind: 'solo', genres: [], location: { city: 'Berlin', country: 'DE' } },
		managers,
		status: 'active',
		revision: 1,
		createdBy: ROOT,
		createdAt: '2026-01-01T00:00:00.000Z',
		updatedAt: '2026-01-01T00:00:00.000Z'
	});

const verification = (subject: string) => ({
	subject,
	handle: 'neon.example',
	displayName: 'Neon Tide',
	createdAt: '2026-01-01T00:00:00.000Z'
});

describe('verification record script', () => {
	it("keeps only the root's verifications, as written", async () => {
		const hv = new FakeHappyView(ROOT);
		const root = await ingest(hv, ROOT, VERIFICATION, '3kroot', verification(DJ));
		const other = await ingest(hv, 'did:plc:other', VERIFICATION, '3kother', verification(DJ));

		expect(await values(hv, [root, other])).toEqual([{ $type: VERIFICATION, ...verification(DJ) }]);
		await hv.ingest({ did: ROOT, collection: VERIFICATION, rkey: '3kroot', cid: '', record: null });
		expect(await stored(hv, [root])).toEqual([]);
	});
});

describe('profile record script', () => {
	const profile = { displayName: 'Neon Tide', description: 'Rolling techno', pronouns: 'they/them' };

	it('keeps the display names of accounts the root verified or that manage its projects, at the key self only', async () => {
		const hv = new FakeHappyView(ROOT);
		await project(hv, 'harbour', ['did:plc:manager']);
		// A project in someone else's repo doesn't make its managers count, nor does their verification.
		await ingest(hv, 'did:plc:other', COLLECTION.project, 'fake', { managers: ['did:plc:faker'] });
		await ingest(hv, 'did:plc:other', VERIFICATION, '3kv', verification('did:plc:faker'));
		await ingest(hv, ROOT, VERIFICATION, '3kv', verification(DJ));

		const uris = [];

		for (const did of [DJ, 'did:plc:manager', 'did:plc:faker', 'did:plc:stranger'])
			uris.push(await ingest(hv, did, BSKY_PROFILE, 'self', profile));
		uris.push(await ingest(hv, DJ, BSKY_PROFILE, 'other', profile));

		expect((await stored(hv, uris)).map((record) => record.uri)).toEqual(uris.slice(0, 2));
		expect(await values(hv, [uris[0]])).toEqual([{ $type: BSKY_PROFILE, displayName: 'Neon Tide' }]);

		await ingest(hv, DJ, BSKY_PROFILE, 'self', { description: 'No name' });
		expect(await values(hv, [uris[0]])).toEqual([{ $type: BSKY_PROFILE }]);
		await hv.ingest({ did: DJ, collection: BSKY_PROFILE, rkey: 'self', cid: '', record: null });
		expect(await stored(hv, [uris[0]])).toEqual([]);
	});
});

describe('follow record script', () => {
	const follow = (subject: string) => ({ subject, createdAt: '2026-01-01T00:00:00.000Z', via: { uri: 'x', cid: 'y' } });

	const epk = (projectUri: string, access: JsonObject) => ({
		project: projectUri,
		title: 'Clubs',
		access,
		createdAt: '2026-01-01T00:00:00.000Z'
	});

	it('keeps the follows of press kit owners whose kit reads them, with just the subject and when', async () => {
		const hv = new FakeHappyView(ROOT);
		const harbour = await project(hv, 'harbour', [DJ, 'did:plc:codj', 'did:plc:same']);
		const reads = { bookedWith: 'auto', following: 'auto', promoters: 'review', anyone: 'review' };
		await ingest(hv, DJ, COLLECTION.epk, 'reads', epk(harbour, reads));
		// The rule only matters where it differs from anyone's.
		await ingest(hv, 'did:plc:same', COLLECTION.epk, 'same', epk(harbour, { ...reads, anyone: 'auto' }));

		const uris = [];

		for (const did of [DJ, 'did:plc:same', 'did:plc:codj', 'did:plc:stranger'])
			uris.push(await ingest(hv, did, BSKY_FOLLOW, '3kfollow', follow('did:plc:friend')));
		uris.push(await ingest(hv, DJ, BSKY_FOLLOW, '3kodd', { subject: 7, createdAt: '2026-01-01' }));

		expect((await stored(hv, uris)).map((record) => record.uri)).toEqual([uris[0]]);
		expect(await values(hv, [uris[0]])).toEqual([{ subject: 'did:plc:friend', createdAt: '2026-01-01T00:00:00.000Z' }]);

		await hv.ingest({ did: DJ, collection: BSKY_FOLLOW, rkey: '3kfollow', cid: '', record: null });
		expect(await stored(hv, uris)).toEqual([]);
	});
});

describe('userinput.app space record script', () => {
	const space = { name: 'Local Network', createdAt: '2026-01-01T00:00:00.000Z' };

	it('keeps only the space bug reports post to, and none without one', async () => {
		const hv = new FakeHappyView(ROOT, happyViewBundle(ROOT, SPACE));
		const ours = await ingest(hv, 'did:plc:userinput', USERINPUT.space, '3kspace', space);
		const other = await ingest(hv, 'did:plc:userinput', USERINPUT.space, '3kother', space);

		expect(await values(hv, [ours, other])).toEqual([{ $type: USERINPUT.space, ...space }]);

		const none = new FakeHappyView(ROOT);
		await ingest(none, 'did:plc:userinput', USERINPUT.space, '3kspace', space);
		expect(await stored(none, [ours])).toEqual([]);
	});
});
