// Property-based tests for the two classic scripts that own persisted state,
// and for the store an imported queue is restored through.
//
// These are places where a wrong answer is silent and permanent: a mark lost
// on import is a track downloaded twice, a timestamp overwritten with a later
// one is a backup that ages its own contents, a queued track let in twice is a
// batch that says it holds more work than it does. The properties here are the
// rules those files promise in their comments, stated so that every generated
// document has to obey them.
//
// Two things about the generators below are deliberate.
//
// They draw tracks from a handful rather than from every "/uploader/track"
// there could be. Every rule about merging is a rule about a track both sides
// have — the earlier of two marks, the newer of two lookups, a track not
// queued twice — and two sets drawn from the whole space never share one (not
// once in a thousand pairs), so those rules went unexercised however many
// runs there were. From a handful, they share one most of the time.
//
// And they build strings out of `fc.string` rather than `fc.stringMatching`.
// A regular expression's arbitrary cost about fifty milliseconds a document,
// which made the round trip below a thirty-second test; a plain string costs a
// fraction of one, and the schema asks nothing of these strings but that they
// are strings.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import * as fc from 'fast-check';
import { loadState, type StateTransfer } from '../helpers/load.ts';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';
import type { QueueEntry } from '$lib/types';

declare const chrome: ChromeMock;

const RUNS = { numRuns: 200 };

const ASYNC_RUNS = { numRuns: 60 };

// The schema's `permalink`: "/uploader/track", no slashes, spaces or query.
// A few tracks by a few uploaders — the last two odd, but still paths.
const PATHS = ['/a/one', '/a/two', '/b/one', '/b/two', '/c-3/x_y', '/dj-ø/tëst'];

const permalink = () => fc.constantFrom(...PATHS);

// Times as a file can carry them: mostly as toISOString writes them, some with
// an offset. Those sort among the rest as text in the wrong order —
// "…T13:00:00+02:00" is an hour before "…T12:00:00.000Z" — and one is the same
// instant as another, written the other way.
const TIMES = [
	'2026-01-01T00:00:00.000Z',
	'2026-09-08T12:00:00.000Z',
	'2026-09-08T14:00:00+02:00',
	'2026-09-08T13:00:00+02:00',
	'2026-09-09T00:00:00Z'
];

const time = () => fc.constantFrom(...TIMES);

// When a lookup was made. A handful again, so that two lookups of one track
// are as often the same age as not.
const lookedUpAt = () => fc.nat({ max: 3 });

// The earlier of the times a track was given, as instants — and of two ways of
// writing one instant, the one it was given first.
const earliest = (...times: (string | undefined)[]) =>
	times.filter((at) => at !== undefined).reduce((first, at) => (Date.parse(at) < Date.parse(first) ? at : first));

const url = () => fc.oneof(fc.constant(''), fc.webUrl());

const marks = () => fc.dictionary(permalink(), time(), { maxKeys: 4 });

// What SoundCloud said about a track. The genre and the cover are newer than
// the format, so a file written before them has neither.
const knownDetails = (path: string) =>
	fc.record(
		{
			id: fc.option(fc.nat(), { nil: null }),
			permalink: fc.constant(path),
			downloadable: fc.boolean(),
			hasDownloadsLeft: fc.boolean(),
			purchaseUrl: url(),
			purchaseTitle: fc.string({ maxLength: 20 }),
			genre: fc.string({ maxLength: 12 }),
			artworkUrl: url(),
			at: lookedUpAt()
		},
		{
			requiredKeys: ['permalink', 'downloadable', 'hasDownloadsLeft', 'purchaseUrl', 'purchaseTitle', 'at']
		}
	);

// What a Likes row shows by itself: the purchase link, and nulls for all the
// row cannot know. The genre and the cover came later, like the known ones'.
const partialDetails = (path: string) =>
	fc.record(
		{
			partial: fc.constant(true),
			id: fc.constant(null),
			permalink: fc.constant(path),
			downloadable: fc.constant(null),
			hasDownloadsLeft: fc.constant(null),
			purchaseUrl: url(),
			purchaseTitle: fc.string({ maxLength: 20 }),
			genre: fc.constant(null),
			artworkUrl: fc.constant('')
		},
		{
			requiredKeys: ['partial', 'permalink', 'downloadable', 'hasDownloadsLeft', 'purchaseUrl', 'purchaseTitle']
		}
	);

const unavailableDetails = () => fc.record({ unavailable: fc.constant(true), at: lookedUpAt() });

const details = () =>
	fc
		.array(fc.tuple(permalink(), fc.boolean()), { maxLength: 4 })
		.chain((rows) =>
			fc.tuple(
				...rows.map(([path, known]) =>
					(known ? knownDetails(path) : unavailableDetails()).map((entry) => [path, entry] as const)
				)
			)
		)
		.map((pairs) => Object.fromEntries(pairs));

// A queued track: the permalink is the only thing it cannot go without, and
// its details are either an answer about the track or a Likes row's partial
// ones. Generated per permalink so an entry never names another track. The
// genre, like and repost are what the page said and nothing else did, null
// where it said nothing — and absent from files written before they were kept.
const queueEntry = () =>
	permalink().chain((path) =>
		fc.record(
			{
				permalink: fc.constant(path),
				title: fc.string({ maxLength: 30 }),
				artist: fc.string({ maxLength: 30 }),
				artworkUrl: url(),
				details: fc.oneof(fc.constant(null), knownDetails(path), partialDetails(path)),
				genre: fc.option(fc.string({ maxLength: 12 }), { nil: null }),
				liked: fc.option(fc.boolean(), { nil: null }),
				reposted: fc.option(fc.boolean(), { nil: null })
			},
			{ requiredKeys: ['permalink'] }
		)
	);

// Each track once, where the first of it was.
const eachOnce = <Entry extends { permalink: string }>(entries: Entry[]) => [
	...new Map(entries.map((e) => [e.permalink, e])).values()
];

// A queue as its store holds one: a track once — the permalink is its identity.
const queue = () => fc.array(queueEntry(), { maxLength: 6 }).map(eachOnce);

// A queue as a file may carry one: the schema lets it name a track twice, and
// the store it is restored through has to take that track once.
const fileQueue = () => fc.array(queueEntry(), { maxLength: 6 });

// A listed track: a queued one plus what only the Manager's list knows — the
// uploader's wording for a purchase link, and where and when the track came in.
const listedEntry = () =>
	fc
		.tuple(
			queueEntry(),
			fc.record(
				{
					domPurchaseTitle: fc.string({ maxLength: 20 }),
					from: fc.string({ maxLength: 30 }),
					at: fc.nat()
				},
				{ requiredKeys: [] }
			)
		)
		.map(([entry, extras]) => ({ ...entry, ...extras }));

// The list holds a track once, for the same reason the queue does.
const listed = () => fc.array(listedEntry(), { maxLength: 6 }).map(eachOnce);

// The schema's `artistPath`: "/uploader", one segment.
const artistPath = () => fc.constantFrom('/a', '/b', '/c-3', '/dj-ø');

const profileLink = () =>
	fc.record(
		{
			network: fc.string({ maxLength: 10 }),
			title: fc.string({ maxLength: 12 }),
			url: fc.webUrl()
		},
		{ requiredKeys: ['url'] }
	);

// What SoundCloud said about an account: only the time is required.
const artistProfile = (path: string) =>
	fc.record(
		{
			id: fc.option(fc.nat(), { nil: null }),
			permalink: fc.constant(path),
			username: fc.string({ maxLength: 12 }),
			fullName: fc.string({ maxLength: 12 }),
			city: fc.string({ maxLength: 12 }),
			country: fc.string({ maxLength: 2 }),
			followers: fc.option(fc.nat(), { nil: null }),
			trackCount: fc.option(fc.nat(), { nil: null }),
			verified: fc.boolean(),
			avatarUrl: url(),
			bio: fc.string({ maxLength: 40 }),
			links: fc.array(profileLink(), { maxLength: 3 }),
			at: fc.nat()
		},
		{ requiredKeys: ['at'] }
	);

// An artist on the artists tool's list: the path is the one thing it cannot
// go without; the profile is what SoundCloud said, a remembered failure, or
// nobody having asked.
const artistEntry = () =>
	artistPath().chain((path) =>
		fc.record(
			{
				permalink: fc.constant(path),
				name: fc.string({ maxLength: 20 }),
				tracks: fc.array(
					fc.record({ permalink: permalink(), title: fc.string({ maxLength: 20 }) }, { requiredKeys: ['permalink'] }),
					{ maxLength: 3 }
				),
				profile: fc.oneof(fc.constant(null), unavailableDetails(), artistProfile(path)),
				from: fc.string({ maxLength: 30 }),
				at: fc.nat()
			},
			{ requiredKeys: ['permalink'] }
		)
	);

const artists = () => fc.array(artistEntry(), { maxLength: 4 });

const prefs = () =>
	fc.record(
		{
			background: fc.boolean(),
			sort: fc.constantFrom('site', 'page'),
			hideDownloaded: fc.boolean()
		},
		{ requiredKeys: [] }
	);

const validDocument = () =>
	fc.record(
		{
			format: fc.constant('toolbox.manager.state'),
			version: fc.constant(1),
			exportedAt: time(),
			downloaded: marks(),
			details: details(),
			listed: listed(),
			artists: artists(),
			queue: fileQueue(),
			prefs: prefs()
		},
		{ requiredKeys: ['format', 'version'] }
	);

let TbxState: StateTransfer;

beforeEach(async () => {
	TbxState = await loadState();
});

describe('the exported state file (state-transfer.js)', () => {
	// `parse` hands back what it was given with the absent members filled in,
	// so this fails exactly when it refuses a file it wrote itself.
	it('reads back every document it writes', () => {
		fc.assert(
			fc.property(validDocument(), (doc) => {
				const parsed = TbxState.parse(TbxState.serialize(doc));
				expect(parsed).toEqual({
					...doc,
					downloaded: doc.downloaded ?? {},
					details: doc.details ?? {},
					listed: doc.listed ?? [],
					artists: doc.artists ?? [],
					queue: doc.queue ?? [],
					prefs: doc.prefs ?? {}
				});
			}),
			RUNS
		);
	});

	// The panel shows `error.message` to the user, so every refusal has to be
	// one this file chose to make — never a TypeError from a shape nobody
	// thought of.
	it('refuses anything it cannot read with a StateError and a message', () => {
		fc.assert(
			fc.property(
				fc.oneof(
					fc.string({ unit: 'binary' }),
					fc.json(),
					validDocument().map((doc) => JSON.stringify(doc)),
					// Valid-looking documents with one member replaced by junk.
					fc
						.tuple(
							validDocument(),
							fc.constantFrom(
								'format',
								'version',
								'exportedAt',
								'downloaded',
								'details',
								'listed',
								'artists',
								'queue',
								'prefs'
							),
							fc.jsonValue({ maxDepth: 2 })
						)
						.map(([doc, key, junk]) => JSON.stringify({ ...doc, [key]: junk }))
				),
				(text) => {
					const outcome = () => {
						try {
							return { parsed: TbxState.parse(text) };
						} catch (error) {
							return { refused: error };
						}
					};

					expect(outcome()).toBeOneOf([
						{
							refused: expect.toSatisfy((error) => error instanceof TbxState.StateError && error.message.length > 5)
						},
						{
							parsed: expect.objectContaining({
								downloaded: expect.any(Object),
								details: expect.any(Object),
								queue: expect.any(Array),
								prefs: expect.any(Object)
							})
						}
					]);
				}
			),
			RUNS
		);
	});

	// Both of the importer's checks that the schema cannot make (see
	// test/state-schema.test.ts): a permalink that has to agree with another one
	// never gets through disagreeing, whatever the two paths are.
	it('never accepts details filed under the wrong track', () => {
		fc.assert(
			fc.property(permalink(), permalink(), (filedUnder, claims) => {
				fc.pre(filedUnder !== claims);

				const doc = {
					format: TbxState.FORMAT,
					version: TbxState.VERSION,
					details: {
						[filedUnder]: {
							permalink: claims,
							downloadable: true,
							hasDownloadsLeft: true,
							purchaseUrl: '',
							purchaseTitle: '',
							at: 1
						}
					}
				};

				expect(() => TbxState.parse(JSON.stringify(doc))).toThrow(TbxState.StateError);
			}),
			RUNS
		);
	});

	it('never accepts a queued track whose details name another', () => {
		fc.assert(
			fc.property(permalink(), permalink(), (queued, claims) => {
				fc.pre(queued !== claims);

				const doc = {
					format: TbxState.FORMAT,
					version: TbxState.VERSION,
					queue: [
						{
							permalink: queued,
							details: {
								permalink: claims,
								downloadable: true,
								hasDownloadsLeft: true,
								purchaseUrl: '',
								purchaseTitle: '',
								at: 1
							}
						}
					]
				};

				expect(() => TbxState.parse(JSON.stringify(doc))).toThrow(TbxState.StateError);
			}),
			RUNS
		);
	});

	it('adds to what is stored and never takes anything away', async () => {
		await fc.assert(
			fc.asyncProperty(marks(), marks(), async (stored, incoming) => {
				globalThis.chrome = createChrome({ storage: { downloaded: { ...stored } } });

				const doc = TbxState.parse(
					JSON.stringify({ format: TbxState.FORMAT, version: TbxState.VERSION, downloaded: incoming })
				);

				const report = await TbxState.apply(doc);

				const after = chrome.testStore().downloaded;

				for (const path of Object.keys(stored)) expect(after).toHaveProperty(path);

				for (const path of Object.keys(incoming)) expect(after).toHaveProperty(path);

				// Every mark keeps the earlier of the two times it could have.
				for (const [path, at] of Object.entries(after)) {
					expect(at).toBe(earliest(stored[path], incoming[path]));
				}

				const added = Object.keys(incoming).filter((path) => !(path in stored));
				expect(report.marks).toBe(added.length);
			}),
			ASYNC_RUNS
		);
	});

	it('takes details only where they are newer than what is known', async () => {
		await fc.assert(
			fc.asyncProperty(details(), details(), async (stored, incoming) => {
				globalThis.chrome = createChrome({ storage: { details: structuredClone(stored) } });

				const doc = TbxState.parse(
					JSON.stringify({ format: TbxState.FORMAT, version: TbxState.VERSION, details: incoming })
				);

				await TbxState.apply(doc);

				const after = chrome.testStore().details;

				for (const [path, entry] of Object.entries(stored)) {
					const arriving = incoming[path];
					const expected = arriving && arriving.at > entry.at ? arriving : entry;
					expect(after[path]).toEqual(expected);
				}

				const arrived = Object.entries(incoming).filter(([key]) => !(key in stored));

				for (const [path, entry] of arrived) expect(after[path]).toEqual(entry);
			}),
			ASYNC_RUNS
		);
	});
});

// The file carries the queue but does not write it: the Manager panel hands
// `doc.queue` to the store that owns it, and that store is where the promise
// lives — everything that was queued is still queued, in the order it was, and
// everything the file carried is queued too, once, whichever side it came
// from. A file re-imported tomorrow must not double anything up.
describe('the queue an import is restored through (queue.svelte.js)', () => {
	// A popup whose queue is `held`, with a module graph of its own so the
	// store's singleton starts where this run wants it.
	const opened = async (held: QueueEntry[]) => {
		vi.resetModules();
		globalThis.chrome = createChrome({ session: { queue: structuredClone(held) } });
		const { queue: store } = await import('../../src/lib/queue.svelte.ts');
		await store.start();

		return store;
	};

	it('appends to the queue and never reorders or drops what is in it', async () => {
		await fc.assert(
			fc.asyncProperty(queue(), fileQueue(), async (held, incoming) => {
				const store = await opened(held);

				const doc = TbxState.parse(
					JSON.stringify({ format: TbxState.FORMAT, version: TbxState.VERSION, queue: incoming })
				);

				const added = await store.add(doc.queue);

				const after = store.entries;
				// What was queued is untouched, and still at the front, in order.
				expect(after.slice(0, held.length)).toEqual(held);
				// Behind it, each track the file carried that was not queued already
				// — once, however often the file names it, where it first does.
				const queued = new Set(held.map((item) => item.permalink));
				const fresh = [...new Set(incoming.map((entry) => entry.permalink))].filter((path) => !queued.has(path));
				expect(after.slice(held.length).map((item) => item.permalink)).toEqual(fresh);
				// And the count reported is the number of tracks really added.
				expect(added).toBe(fresh.length);
			}),
			ASYNC_RUNS
		);
	});

	// The same file read twice is the same queue: importing is repeatable, as
	// merging the marks is.
	it('adds nothing the second time the same file is imported', async () => {
		await fc.assert(
			fc.asyncProperty(queue(), fileQueue(), async (held, incoming) => {
				const store = await opened(held);

				const doc = TbxState.parse(
					JSON.stringify({ format: TbxState.FORMAT, version: TbxState.VERSION, queue: incoming })
				);

				await store.add(doc.queue);
				const once = store.entries;

				expect(await store.add(doc.queue)).toBe(0);
				expect(store.entries).toEqual(once);
			}),
			ASYNC_RUNS
		);
	});
});

describe('the marks (downloaded-store.js)', () => {
	// What is marked, and then what is unmarked: mostly some of what was marked
	// — the case that has to take a mark away — and now and then a track that
	// never was.
	const markings = () =>
		fc.uniqueArray(permalink(), { maxLength: 6 }).chain((mark) =>
			fc.tuple(
				fc.constant(mark),
				fc
					.tuple(fc.subarray(mark), fc.uniqueArray(permalink(), { maxLength: 2 }))
					.map(([some, others]) => [...new Set([...some, ...others])])
			)
		);

	it('marks and unmarks any set of tracks', async () => {
		await fc.assert(
			fc.asyncProperty(markings(), async ([mark, unmark]) => {
				globalThis.chrome = createChrome();
				await TbxDownloaded.setMany(mark, true);
				await TbxDownloaded.setMany(unmark, false);
				const held = await TbxDownloaded.load();

				for (const path of mark) {
					expect(path in held).toBe(!unmark.includes(path));
				}

				for (const path of unmark) expect(held).not.toHaveProperty(path);
			}),
			ASYNC_RUNS
		);
	});

	it('merges idempotently, keeping the earliest time it has ever seen', async () => {
		await fc.assert(
			fc.asyncProperty(marks(), marks(), async (first, second) => {
				globalThis.chrome = createChrome();
				await TbxDownloaded.merge(first);
				await TbxDownloaded.merge(second);
				const once = await TbxDownloaded.load();

				expect(await TbxDownloaded.merge(first)).toBe(0);
				expect(await TbxDownloaded.merge(second)).toBe(0);
				expect(await TbxDownloaded.load()).toEqual(once);

				for (const path of new Set([...Object.keys(first), ...Object.keys(second)])) {
					expect(once[path]).toBe(earliest(first[path], second[path]));
				}
			}),
			ASYNC_RUNS
		);
	});
});
