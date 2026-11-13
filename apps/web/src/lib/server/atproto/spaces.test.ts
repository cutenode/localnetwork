import * as fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import {
	bookingSpace,
	parsePublicRecordUri,
	parseSpaceRecordUri,
	parseSpaceUri,
	plannerSpace,
	publicRecordUri,
	rkeyOf,
	rootSpaces,
	sharedSpace,
	sharedSpaceKind,
	spaceOfRecord,
	spaceRecordUri,
	spaceUri
} from './spaces';

const ROOT = 'did:plc:root123';

// Valid DIDs, NSIDs and record keys by the AT Protocol's syntax, for the round trip below.
const did = fc.oneof(
	fc.stringMatching(/^did:plc:[a-z2-7]{24}$/),
	fc.stringMatching(/^did:web:[a-z0-9-]{1,20}\.[a-z]{2,6}$/)
);

const nsid = fc.stringMatching(/^[a-z][a-z0-9-]{0,10}[a-z0-9](\.[a-z0-9]{1,10}){1,3}\.[a-zA-Z][a-zA-Z0-9]{0,15}$/);

const key = fc.stringMatching(/^[a-zA-Z0-9_~.:-]{1,40}$/).filter((value) => value !== '.' && value !== '..');

describe('space URIs', () => {
	it('builds and parses space refs', () => {
		const uri = spaceUri(ROOT, 'computer.localnetwork.space.audit', 'main');
		expect(uri).toBe('at://did:plc:root123/space/computer.localnetwork.space.audit/main');
		expect(parseSpaceUri(uri)).toEqual({ authority: ROOT, type: 'computer.localnetwork.space.audit', skey: 'main' });
		expect(parseSpaceUri('at://did:plc:x/app.bsky.feed.post/1')).toBeNull();
	});

	it('builds and parses space record URIs', () => {
		const space = rootSpaces(ROOT).moderation;
		const uri = spaceRecordUri(space, 'did:plc:alice', 'computer.localnetwork.moderation.request', '3abc');
		expect(parseSpaceRecordUri(uri)).toMatchObject({ authority: ROOT, author: 'did:plc:alice', rkey: '3abc' });
		expect(spaceOfRecord(uri)).toBe(space);
		expect(spaceOfRecord('nope')).toBeNull();
	});

	it('handles public record URIs and refuses space URIs there', () => {
		const uri = publicRecordUri(ROOT, 'computer.localnetwork.project', 'abc');
		expect(parsePublicRecordUri(uri)).toEqual({ did: ROOT, collection: 'computer.localnetwork.project', rkey: 'abc' });
		expect(parsePublicRecordUri(`at://${ROOT}/space/x`)).toBeNull();
	});

	it('parses back every space, space record and public record URI it builds, each only as its own kind', () => {
		fc.assert(
			fc.property(did, nsid, key, did, nsid, key, (authority, type, skey, author, collection, rkey) => {
				const space = spaceUri(authority, type, skey);
				const record = spaceRecordUri(space, author, collection, rkey);
				const pub = publicRecordUri(author, collection, rkey);

				expect([parseSpaceUri(space), parseSpaceRecordUri(space), parsePublicRecordUri(space)]).toEqual([
					{ authority, type, skey },
					null,
					null
				]);
				expect([parseSpaceUri(record), parseSpaceRecordUri(record), parsePublicRecordUri(record)]).toEqual([
					null,
					{ authority, type, skey, author, collection, rkey },
					null
				]);
				expect([parseSpaceUri(pub), parseSpaceRecordUri(pub), parsePublicRecordUri(pub)]).toEqual([
					null,
					null,
					{ did: author, collection, rkey }
				]);
			})
		);
	});

	it('refuses URIs that are not valid record or space URIs', () => {
		const space = `at://${ROOT}/space/computer.localnetwork.space.audit/main`;
		const record = `${space}/did:plc:alice/computer.localnetwork.moderation.request/3abc`;
		const pub = `at://${ROOT}/computer.localnetwork.project/abc`;

		for (const [parse, uri] of [
			[parseSpaceUri, `at://${ROOT}/space/x/main`],
			[parseSpaceUri, `at://${ROOT}/space/computer.localnetwork.space.audit/..`],
			[parseSpaceUri, `at://alice.test/space/computer.localnetwork.space.audit/main`],
			[parseSpaceUri, `${space}/`],
			[parseSpaceUri, `${space}?x=1`],
			[parseSpaceUri, `${space}#/x`],
			[parseSpaceRecordUri, `${space}/did:plc:alice/request/3abc`],
			[parseSpaceRecordUri, `${space}/alice.test/computer.localnetwork.moderation.request/3abc`],
			[parseSpaceRecordUri, `${record}/more`],
			[parseSpaceRecordUri, `${record}#/x`],
			[parsePublicRecordUri, `at://alice.test/computer.localnetwork.project/abc`],
			[parsePublicRecordUri, `at://${ROOT}/project/abc`],
			[parsePublicRecordUri, `at://${ROOT}/computer.localnetwork.project/.`],
			[parsePublicRecordUri, `at://${ROOT}/computer.localnetwork.project`],
			[parsePublicRecordUri, `${pub}#/x`],
			[parsePublicRecordUri, `${pub} `]
		] as const)
			expect(parse(uri), uri).toBeNull();
	});

	it('refuses `.` and `..` as record and space keys, which atproto never allows', () => {
		const space = rootSpaces(ROOT).moderation;

		for (const key of ['.', '..']) {
			expect(parsePublicRecordUri(publicRecordUri(ROOT, 'computer.localnetwork.dj.set', key))).toBeNull();
			expect(parseSpaceUri(`at://${ROOT}/space/computer.localnetwork.space.audit/${key}`)).toBeNull();
			expect(
				parseSpaceRecordUri(spaceRecordUri(space, 'did:plc:alice', 'computer.localnetwork.moderation.request', key))
			).toBeNull();
			expect(
				parseSpaceRecordUri(`at://${ROOT}/space/computer.localnetwork.space.audit/${key}/did:plc:alice/a.b.c/3abc`)
			).toBeNull();
		}

		// Dots inside a key are fine.
		expect(parsePublicRecordUri(publicRecordUri(ROOT, 'computer.localnetwork.dj.set', '...'))).toMatchObject({
			rkey: '...'
		});
	});

	it("takes a record URI's rkey", () => {
		expect(rkeyOf(publicRecordUri(ROOT, 'app.bsky.graph.verification', '3kverify'))).toBe('3kverify');
		expect(rkeyOf('self')).toBe('self');
		expect(rkeyOf(`at://${ROOT}/`)).toBe('');
	});

	it('always takes the last segment between slashes', () => {
		fc.assert(
			fc.property(fc.array(fc.string()), fc.string(), (head, last) => {
				const segment = last.replaceAll('/', '');

				expect(rkeyOf([...head, segment].join('/'))).toBe(segment);
			})
		);
	});

	it('names root, booking, and planner spaces', () => {
		expect(Object.keys(rootSpaces(ROOT))).toEqual(['audit', 'moderation', 'staff', 'industry']);
		expect(bookingSpace(ROOT, 'tid1')).toBe(`at://${ROOT}/space/computer.localnetwork.space.booking/tid1`);
		expect(plannerSpace('did:plc:pat')).toBe('at://did:plc:pat/space/computer.localnetwork.space.planner/self');
	});

	it("names performers', parties' and venues' spaces, and tells their kind from the type", () => {
		const party = sharedSpace(ROOT, 'party', 'tid1');

		expect(party).toBe(`at://${ROOT}/space/computer.localnetwork.space.party/tid1`);
		expect(sharedSpaceKind(parseSpaceUri(party)!.type)).toBe('party');
		expect(sharedSpaceKind('computer.localnetwork.space.venue')).toBe('venue');
		expect(sharedSpaceKind('computer.localnetwork.space.backstage')).toBeNull();
	});
});
