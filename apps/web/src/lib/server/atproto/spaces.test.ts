import * as fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { AtUri, SpaceRef } from '@atproto/syntax';
import {
	writtenAs,
	bookingSpace,
	currentSpaceUri,
	currentSpaceUris,
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
		expect(uri).toBe('at:space/did:plc:root123/computer.localnetwork.space.audit/main');
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
		expect(parsePublicRecordUri(rootSpaces(ROOT).audit)).toBeNull();
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

	describe('on any string', () => {
		const validUri = fc.oneof(
			fc.tuple(did, nsid, key).map(([authority, type, skey]) => spaceUri(authority, type, skey)),
			fc
				.tuple(did, nsid, key, did, nsid, key)
				.map(([authority, type, skey, author, collection, rkey]) =>
					spaceRecordUri(spaceUri(authority, type, skey), author, collection, rkey)
				),
			fc.tuple(did, nsid, key).map(([author, collection, rkey]) => publicRecordUri(author, collection, rkey))
		);

		/** What URIs from other repos and XRPC bodies look like when they're off by a little: a valid one with one edit. */
		const nearUri = fc
			.tuple(
				validUri,
				fc.nat(),
				fc.nat({ max: 3 }),
				fc.oneof(
					fc.constantFrom(
						'/',
						'//',
						'?',
						'?x=1',
						'#',
						'#/x',
						'%2F',
						'%',
						' ',
						'\n',
						'.',
						'..',
						':',
						'@',
						'A',
						'did:plc:'
					),
					fc.string({ maxLength: 4 })
				)
			)
			.map(([uri, at, cut, insert]) => {
				const index = at % (uri.length + 1);

				return uri.slice(0, index) + insert + uri.slice(index + cut);
			});

		// A fragment the AT URI syntax allows, which an edit anywhere in the URI seldom makes.
		const withFragment = validUri.map((uri) => `${uri}#/fragment`);

		const anyUri = fc.oneof(nearUri, withFragment, fc.string(), fc.string({ unit: 'binary' }), fc.webUrl());

		it('parses a URI only when its parts build it back exactly, and never throws', () => {
			fc.assert(
				fc.property(anyUri, (uri) => {
					const space = parseSpaceUri(uri);
					const record = parseSpaceRecordUri(uri);
					const pub = parsePublicRecordUri(uri);

					const rebuilt = [
						space ? spaceUri(space.authority, space.type, space.skey) : null,
						record
							? spaceRecordUri(
									spaceUri(record.authority, record.type, record.skey),
									record.author,
									record.collection,
									record.rkey
								)
							: null,
						pub ? publicRecordUri(pub.did, pub.collection, pub.rkey) : null
					].filter((built) => built !== null);

					expect(rebuilt).toEqual(rebuilt.map(() => uri));
					// A URI is one kind or none.
					expect(rebuilt.length).toBeLessThanOrEqual(1);
				})
			);
		});
	});

	it('refuses URIs that are not valid record or space URIs', () => {
		const space = `at:space/${ROOT}/computer.localnetwork.space.audit/main`;
		const record = `${space}/did:plc:alice/computer.localnetwork.moderation.request/3abc`;
		const pub = `at://${ROOT}/computer.localnetwork.project/abc`;

		for (const [parse, uri] of [
			[parseSpaceUri, `at:space/${ROOT}/x/main`],
			[parseSpaceUri, `at:space/${ROOT}/computer.localnetwork.space.audit/..`],
			[parseSpaceUri, `at:space/alice.test/computer.localnetwork.space.audit/main`],
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

	it('refuses the form the alpha wrote them in', () => {
		expect(parseSpaceUri(`at://${ROOT}/space/computer.localnetwork.space.audit/main`)).toBeNull();
		expect(
			parseSpaceRecordUri(`at://${ROOT}/space/computer.localnetwork.space.audit/main/did:plc:alice/a.b.c/3abc`)
		).toBeNull();
	});

	it('refuses `.` and `..` as record and space keys, which atproto never allows', () => {
		const space = rootSpaces(ROOT).moderation;

		for (const key of ['.', '..']) {
			expect(parsePublicRecordUri(publicRecordUri(ROOT, 'computer.localnetwork.dj.set', key))).toBeNull();
			expect(parseSpaceUri(`at:space/${ROOT}/computer.localnetwork.space.audit/${key}`)).toBeNull();
			expect(
				parseSpaceRecordUri(spaceRecordUri(space, 'did:plc:alice', 'computer.localnetwork.moderation.request', key))
			).toBeNull();
			expect(
				parseSpaceRecordUri(`at:space/${ROOT}/computer.localnetwork.space.audit/${key}/did:plc:alice/a.b.c/3abc`)
			).toBeNull();
		}

		// Dots inside a key are fine.
		expect(parsePublicRecordUri(publicRecordUri(ROOT, 'computer.localnetwork.dj.set', '...'))).toMatchObject({
			rkey: '...'
		});
	});

	it('builds the URIs @atproto/syntax builds, and reads them back', () => {
		fc.assert(
			fc.property(did, nsid, key, did, nsid, key, (authority, type, skey, author, collection, rkey) => {
				const space = spaceUri(authority, type, skey);
				const record = spaceRecordUri(space, author, collection, rkey);

				expect(SpaceRef.parse(space)).toMatchObject({ spaceDid: authority, spaceType: type, skey });
				expect(new AtUri(record)).toMatchObject({ isSpace: true, authorDid: author, collection, rkey });
			})
		);
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
		expect(bookingSpace(ROOT, 'tid1')).toBe(`at:space/${ROOT}/computer.localnetwork.space.booking/tid1`);
		expect(plannerSpace('did:plc:pat')).toBe('at:space/did:plc:pat/computer.localnetwork.space.planner/self');
	});

	it("rewrites the alpha's space and record URIs, and nothing else", () => {
		fc.assert(
			fc.property(did, nsid, key, did, nsid, key, (authority, type, skey, author, collection, rkey) => {
				const space = spaceUri(authority, type, skey);
				const record = spaceRecordUri(space, author, collection, rkey);
				const pub = publicRecordUri(author, collection, rkey);
				const alpha = (uri: string) => uri.replace(`at:space/${authority}/`, `at://${authority}/space/`);

				expect([alpha(space), alpha(record)].map(currentSpaceUri)).toEqual([space, record]);
				expect([space, record, pub, author].map(currentSpaceUri)).toEqual([space, record, pub, author]);
			})
		);
	});

	it('leaves any string it has rewritten, or that is something else, as it is', () => {
		fc.assert(
			fc.property(fc.oneof(fc.string(), fc.webUrl(), fc.string({ unit: 'binary' })), (text) => {
				expect(currentSpaceUri(currentSpaceUri(text))).toBe(currentSpaceUri(text));
			})
		);
		expect(currentSpaceUri('see at://did:plc:a/space/x/y')).toBe('see at://did:plc:a/space/x/y');
		expect(currentSpaceUri('at://alice.test/space/x/y')).toBe('at://alice.test/space/x/y');

		for (const text of [
			'at://did:plc:a/space/x/y is the one',
			'the one is "at://did:plc:a/space/x/y"',
			'"at://did:plc:a/space/x/y',
			'at://did:plc:a/space/x/"y'
		])
			expect(currentSpaceUris({ text })).toEqual({ text });
	});

	it("rewrites a record's URIs wherever they are in it", () => {
		const alpha = `at://${ROOT}/space/computer.localnetwork.space.venue/3v`;
		const now = sharedSpace(ROOT, 'venue', '3v');
		const record = { venue: alpha, list: [alpha, { deep: alpha }], count: 1, none: null, text: 'hi' };

		expect(currentSpaceUris(record)).toEqual({
			venue: now,
			list: [now, { deep: now }],
			count: 1,
			none: null,
			text: 'hi'
		});
	});

	it("names a space as an audit entry might: now, and in the alpha's form", () => {
		const space = rootSpaces(ROOT).audit;

		expect(writtenAs(space)).toEqual([space, `at://${ROOT}/space/computer.localnetwork.space.audit/main`]);
		expect(writtenAs(publicRecordUri(ROOT, 'a.b.c', '1'))).toEqual([publicRecordUri(ROOT, 'a.b.c', '1')]);
		fc.assert(
			fc.property(did, nsid, key, (authority, type, skey) => {
				const uri = spaceUri(authority, type, skey);

				expect(writtenAs(uri).map(currentSpaceUri)).toEqual([uri, uri]);
			})
		);
	});

	it("names performers', parties' and venues' spaces, and tells their kind from the type", () => {
		const party = sharedSpace(ROOT, 'party', 'tid1');

		expect(party).toBe(`at:space/${ROOT}/computer.localnetwork.space.party/tid1`);
		expect(sharedSpaceKind(parseSpaceUri(party)!.type)).toBe('party');
		expect(sharedSpaceKind('computer.localnetwork.space.venue')).toBe('venue');
		expect(sharedSpaceKind('computer.localnetwork.space.backstage')).toBeNull();
	});
});
