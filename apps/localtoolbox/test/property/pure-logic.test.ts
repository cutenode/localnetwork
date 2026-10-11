// Property-based tests for the pure logic.
//
// The example tests beside these say what each function does for the inputs
// the extension actually meets. These say what must hold for *every* input,
// which is a different question and catches a different kind of mistake: a
// title with a comma in it, a permalink with a lone surrogate, a gate URL
// wrapped in one more layer of punctuation than anyone thought to write down.
//
// Two kinds of property appear here:
//
//   robustness — fed arbitrary bytes, the function answers rather than throws.
//     These use `unit: 'binary'`, which includes lone surrogates and control
//     characters, because that is what a page can really contain.
//   round-trip — a value put through the encoder and back comes out equal.
//     These are the strongest statements in the suite: they pin the whole
//     shape of the output, not the parts a test author thought to assert.
//
// A failure prints the smallest input that reproduces it (fast-check shrinks),
// which is usually the bug report as well.
import { describe, it, expect } from 'vitest';
import * as fc from 'fast-check';
import type { DetailsAnswer, ListedEntry, TracksAnswer } from '$lib/types';

import { normalize, matches } from '../../src/lib/filter.ts';
import { count, pageSummary, emptyStatus } from '../../src/lib/soundcloud.ts';
import {
	FORMATS,
	formatFor,
	processTracks,
	splitTitleArtist,
	dedupeTracks,
	slugify,
	type Line
} from '../../src/tools/soundcloud-playlist-to-text/formats.ts';
import { artworkUrlAtSize, FULL_SIZE } from '../../src/lib/artwork.ts';
import {
	unwrapGate,
	hostOf,
	domainOf,
	entriesFor,
	splitEntries,
	buildGroups,
	buildListOrder,
	isUnchecked,
	isWithoutLinks,
	lookupNotes
} from '../../src/tools/manager/links.ts';
import { safeName, filenameOf, looksLikeFile } from '../../src/tools/third-party-downloads/save.ts';
import {
	jsonAfter,
	flightPayload,
	decodeBase64Json,
	documentOf,
	formFields
} from '../../src/tools/third-party-downloads/parse.ts';
import {
	siteFor,
	isGateUrl,
	extractGateUrls,
	labelOfUrl,
	SITES,
	HOSTS,
	CLAIMED,
	unwrapGate as unwrapGateSites
} from '../../src/tools/third-party-downloads/sites/index.ts';

// Enough runs to be worth the name, few enough to keep the suite quick.
const RUNS = { numRuns: 300 };

// The characters that mean something to the code under test: whitespace to
// collapse, separators to cut at, punctuation to quote or strip.
const PUNCTUATION = [...Array.from(' \t\n\r /\\.,"\'<>-?#%aZé'), '  ', '..', ' - ', ' – ', ' — '];

// Anything a page can hold: control characters, lone surrogates, the lot.
// `unit: 'binary'` alone is every code point, and spread over all of them it
// almost never lands on the few above — in 300 runs it wrote no space, slash,
// dot, comma, quote or " - " at all — so a property fed nothing else never
// meets the input it is about. Hence the other two.
const bytes = (options = {}) =>
	fc.oneof(
		fc.string({ unit: 'binary', ...options }),
		fc.string({ unit: 'grapheme', ...options }),
		fc.string({ unit: fc.constantFrom(...PUNCTUATION), ...options })
	);

// Text a person would type or a title would contain.
const words = () => fc.string({ unit: fc.constantFrom(...'abcdefgh ÉÈéè-_0123'.split('')), maxLength: 24 });

// A URL path made only of characters that never end a link in prose, so a
// generated URL is the whole link and not a prefix of one.
const safePath = () =>
	fc
		.array(fc.stringMatching(/^[a-zA-Z0-9_~-]{1,10}$/), { minLength: 1, maxLength: 3 })
		.map((parts) => `/${parts.join('/')}`);

const trackOf = (artist: fc.Arbitrary<string>, title: fc.Arbitrary<string>) => fc.record({ artist, title });

const anyTrack = () => trackOf(bytes(), bytes());

const anyTracks = () => fc.array(anyTrack(), { maxLength: 12 });

// ---------------------------------------------------------------------------

describe('the search box (filter.js)', () => {
	it('normalizes anything without throwing, into single-spaced trimmed text', () => {
		fc.assert(
			fc.property(bytes(), (text) => {
				const out = normalize(text);
				expect(out).toBeTypeOf('string');
				expect(out).not.toMatch(/^\s|\s$|\s\s/);
			}),
			RUNS
		);
	});

	// The panels pass whatever a track carries, which is a string or nothing;
	// these are the shapes a missing or half-read field really arrives as.
	it('normalizes a missing or non-string field into text', () => {
		fc.assert(
			fc.property(
				fc.oneof(
					fc.constant(null),
					fc.constant(undefined),
					fc.double(),
					fc.boolean(),
					fc.array(bytes(), { maxLength: 3 })
				),
				(value) => {
					// @ts-expect-error -- a number, a boolean or an array where a field's text belongs
					expect(normalize(value)).toBeTypeOf('string');
				}
			),
			RUNS
		);
	});

	it('matches every text against the empty query, and every text against itself', () => {
		fc.assert(
			fc.property(bytes(), (text) => {
				expect(matches(text, '')).toBe(true);
				expect(matches(text, '   ')).toBe(true);
				expect(matches(text, text)).toBe(true);
			}),
			RUNS
		);
	});

	// "glue bicep" finds "Bicep — Glue": the words of a query are looked for one
	// by one, not as a phrase, so any of a text's own words, in any order, find it.
	it('matches any of a text’s own words, in any order', () => {
		fc.assert(
			fc.property(
				fc
					.array(words(), { minLength: 1, maxLength: 5 })
					.chain((parts) => fc.tuple(fc.constant(parts.join(' ')), fc.shuffledSubarray(parts))),
				([text, picked]) => {
					expect(matches(text, picked.join(' '))).toBe(true);
				}
			),
			RUNS
		);
	});

	it('does not lose a match when more text is added around it', () => {
		fc.assert(
			fc.property(words(), words(), words(), (before, text, after) => {
				fc.pre(matches(text, text) && normalize(text) !== '');
				expect(matches(`${before} ${text} ${after}`, text)).toBe(true);
			}),
			RUNS
		);
	});

	it('ignores case and accents', () => {
		fc.assert(
			fc.property(words(), (text) => {
				fc.pre(normalize(text) !== '');
				expect(matches(text.toUpperCase(), normalize(text))).toBe(true);
			}),
			RUNS
		);
	});
});

// ---------------------------------------------------------------------------

describe('counting and summarising (soundcloud.js)', () => {
	it('pluralises every whole number but one', () => {
		fc.assert(
			fc.property(fc.integer({ min: -50, max: 5000 }), (n) => {
				expect(count(n, 'track')).toBe(`${n} ${n === 1 ? 'track' : 'tracks'}`);
			}),
			RUNS
		);
	});

	it('summarises any page without throwing, always naming the track count', () => {
		fc.assert(
			fc.property(
				fc.record({
					title: fc.option(bytes(), { nil: '' }),
					curator: fc.option(bytes(), { nil: '' }),
					tracks: fc.array(fc.constant({}), { maxLength: 30 }),
					skippedPlaylists: fc.nat({ max: 20 })
				}),
				fc.array(bytes(), { maxLength: 3 }),
				(page, extra) => {
					// SAFETY: pageSummary reads only the title, the curator, how many tracks
					// there are and skippedPlaylists, which is all of what the record makes.
					const summary = pageSummary(page as TracksAnswer, extra);
					expect(summary).toContain(count(page.tracks.length, 'track'));
					// Skipped playlists are said, and only when there are some.
					expect(summary.includes('skipped')).toBe(page.skippedPlaylists > 0);
				}
			),
			RUNS
		);
	});

	it('always has something to say about a page with no tracks', () => {
		fc.assert(
			fc.property(
				fc.record({
					pageType: fc.constantFrom('playlist', 'likes', 'other', '', 'unknown'),
					skippedPlaylists: fc.nat({ max: 5 })
				}),
				(response) => {
					// @ts-expect-error -- page types this version never names, and only the fields it reads
					expect(emptyStatus(response).length).toBeGreaterThan(20);
				}
			),
			RUNS
		);
	});
});

// ---------------------------------------------------------------------------

// RFC 4180 enough for what csvField() writes: quoted fields, doubled quotes.
// Outside quotes a line ends at \n, \r\n or a bare \r, as a spreadsheet reads
// it — so a line break csvField failed to quote shows up as an extra row.
function parseCsv(text: string) {
	const rows = [];
	let row = [];
	let field = '';
	let quoted = false;

	for (let i = 0; i < text.length; i += 1) {
		const char = text[i];

		if (quoted) {
			if (char !== '"') field += char;
			else if (text[i + 1] === '"') {
				field += '"';
				i += 1;
			} else quoted = false;
		} else if (char === '"' && field === '') quoted = true;
		else if (char === ',') {
			row.push(field);
			field = '';
		} else if (char === '\n' || char === '\r') {
			if (char === '\r' && text[i + 1] === '\n') i += 1;
			row.push(field);
			rows.push(row);
			row = [];
			field = '';
		} else field += char;
	}

	row.push(field);
	rows.push(row);

	return rows;
}

describe('the text formats (formats.js)', () => {
	it('renders every format for any tracks, as a string', () => {
		fc.assert(
			fc.property(anyTracks(), (tracks) => {
				for (const format of FORMATS) expect(format.render(tracks)).toBeTypeOf('string');
			}),
			RUNS
		);
	});

	it('renders a header alone, or nothing at all, for no tracks', () => {
		expect(Object.fromEntries(FORMATS.map((format) => [format.value, format.render([])]))).toEqual({
			'artist-title': '',
			'title-artist': '',
			'title-only': '',
			numbered: '',
			csv: 'Artist,Title',
			tsv: 'Artist\tTitle',
			json: '[]'
		});
	});

	// Text with what CSV has to quote in it — commas, quotes and line breaks of
	// both kinds — as well as fc.string()'s printable ASCII, which never has a
	// line break.
	const csvText = () => fc.oneof(fc.string(), fc.string({ unit: fc.constantFrom(...Array.from('a é,";\n\r')) }));

	it('round-trips artist and title through CSV, whatever they contain', () => {
		fc.assert(
			fc.property(fc.array(trackOf(csvText(), csvText()), { maxLength: 10 }), (tracks) => {
				const csv = formatFor('csv').render(tracks);
				const [header, ...rows] = parseCsv(csv);
				expect(header).toEqual(['Artist', 'Title']);
				expect(rows).toEqual(tracks.map((t) => [t.artist, t.title]));
			}),
			RUNS
		);
	});

	it('round-trips artist and title through JSON, whatever they contain', () => {
		fc.assert(
			fc.property(anyTracks(), (tracks) => {
				const parsed = JSON.parse(formatFor('json').render(tracks));
				expect(parsed).toEqual(tracks.map(({ artist, title }) => ({ artist, title })));
			}),
			RUNS
		);
	});

	it('numbers every track exactly once, in order', () => {
		fc.assert(
			fc.property(fc.array(anyTrack(), { minLength: 1, maxLength: 20 }), (tracks) => {
				const lines = formatFor('numbered')
					.render(tracks)
					.split(/\r\n|\r|\n/);

				expect(lines).toHaveLength(tracks.length);
				lines.forEach((line, i) => expect(line.startsWith(`${i + 1}. `)).toBe(true));
			}),
			RUNS
		);
	});

	// Whatever a restored queue put in a title, a line format is a track a line
	// and TSV a track a row of exactly two fields.
	it('keeps a track to a line, and TSV to two fields, whatever the text holds', () => {
		fc.assert(
			fc.property(fc.array(anyTrack(), { minLength: 1, maxLength: 12 }), (tracks) => {
				for (const value of ['artist-title', 'title-artist', 'title-only', 'numbered']) {
					expect(
						formatFor(value)
							.render(tracks)
							.split(/\r\n|\r|\n/),
						value
					).toHaveLength(tracks.length);
				}

				const rows = formatFor('tsv')
					.render(tracks)
					.split(/\r\n|\r|\n/);

				expect(rows).toHaveLength(tracks.length + 1);

				for (const row of rows) expect(row.split('\t')).toHaveLength(2);
			}),
			RUNS
		);
	});

	it('falls back to the first format for any value it does not know', () => {
		fc.assert(
			fc.property(bytes(), (value) => {
				fc.pre(!FORMATS.some((f) => f.value === value));
				expect(formatFor(value)).toBe(FORMATS[0]);
			}),
			RUNS
		);
	});

	it('splits a title at most into two halves, both taken out of the title', () => {
		fc.assert(
			fc.property(anyTrack(), (track) => {
				const split = splitTitleArtist(track);
				expect(split.artist).toBeTypeOf('string');
				expect(split.title).toBeTypeOf('string');

				// Nothing is invented: a title it split has both halves in it.
				const halves = split === track ? [] : [split.artist, split.title];

				for (const half of halves) expect(track.title).toContain(half);
			}),
			RUNS
		);
	});

	// Every separator starts with a space, so a first half with no space in it
	// cannot hold one, and the title splits exactly where the two were joined.
	it('splits "Artist - Title" where the separator is, whichever dash it uses', () => {
		fc.assert(
			fc.property(
				bytes().map((text) => text.replace(/ /g, '')),
				fc.constantFrom(' - ', ' – ', ' — '),
				bytes(),
				(artist, separator, title) => {
					fc.pre(artist !== '' && title !== '');
					const track = { artist: 'Channel', title: `${artist}${separator}${title}` };
					expect(splitTitleArtist(track)).toEqual({ artist: artist.trim(), title: title.trim() });
				}
			),
			RUNS
		);
	});

	it('dedupes to a subsequence, idempotently, keeping one of each track', () => {
		// Few enough names that a list repeats itself, in either case, and puts
		// the artist/title boundary in different places: "a" + "b c" and "a b" +
		// "c" are two tracks, not one.
		const name = () => fc.constantFrom('a', 'A', 'a b', 'b', 'b c', 'B C', 'c', '');
		fc.assert(
			fc.property(fc.array(trackOf(name(), name()), { maxLength: 12 }), (tracks) => {
				const once = dedupeTracks(tracks);
				// A subsequence: same objects, same order, nothing added.
				expect(once).toEqual(tracks.filter((t) => once.includes(t)));
				expect(dedupeTracks(once)).toEqual(once);
				// The first of each track and no other, a track being its artist and
				// its title, case aside — held apart as a pair, so that no delimiter
				// can blur where the one ends and the other begins.
				const pair = (t: Line) => JSON.stringify([t.artist.toLowerCase(), t.title.toLowerCase()]);
				expect(once.map(pair)).toEqual([...new Set(tracks.map(pair))]);
			}),
			RUNS
		);
	});

	it('processes tracks under every combination of options', () => {
		fc.assert(
			fc.property(anyTracks(), fc.boolean(), fc.boolean(), (tracks, splitTitle, dedupe) => {
				const out = processTracks(tracks, { splitTitle, dedupe });
				expect(Array.isArray(out)).toBe(true);
				expect(out.length).toBeLessThanOrEqual(tracks.length);
				// With neither option on, the tracks come through as they were.
				expect(processTracks(tracks, { splitTitle: false, dedupe: false })).toEqual(tracks);
			}),
			RUNS
		);
	});

	// A queued track may name neither (see QueueEntry): absent is what '' is.
	it('reads a missing artist or title as empty, under every option and format', () => {
		fc.assert(
			fc.property(
				fc.array(fc.record({ artist: bytes(), title: bytes() }, { requiredKeys: [] }), {
					maxLength: 12
				}),
				fc.boolean(),
				fc.boolean(),
				(tracks, splitTitle, dedupe) => {
					const blank = tracks.map(({ artist = '', title = '' }) => ({ artist, title }));
					const options = { splitTitle, dedupe };

					for (const format of FORMATS) {
						expect(format.render(processTracks(tracks, options)), format.value).toBe(
							format.render(processTracks(blank, options))
						);
					}
				}
			),
			RUNS
		);
	});

	it('slugifies anything into a usable filename stem', () => {
		fc.assert(
			fc.property(bytes(), (title) => {
				const slug = slugify(title);
				expect(slug).toMatch(/^[a-z0-9]+(-[a-z0-9]+)*$/);
				expect(slug.length).toBeGreaterThan(0);
			}),
			RUNS
		);
	});
});

// ---------------------------------------------------------------------------

const artworkUrl = () =>
	fc
		.tuple(
			fc.constantFrom('i1', 'i2', 'a1'),
			fc.stringMatching(/^[0-9a-f]{1,12}$/),
			fc.constantFrom('t120x120', 't500x500', 't1080x1080', 'original', 'large', 'crop'),
			fc.constantFrom('jpg', 'png', 'gif', 'jpeg')
		)
		.map(([sub, hash, size, ext]) => `https://${sub}.sndcdn.com/artworks-${hash}-${size}.${ext}`);

// The sizes anything asks for: the one the panel uses, and a couple more to
// show the rewrite is not special to it.
const SIZES = fc.constantFrom(FULL_SIZE, 't500x500', 't120x120');

describe('artwork URLs (artwork.js)', () => {
	it('rewrites only the size token, and always to the size asked for', () => {
		fc.assert(
			fc.property(artworkUrl(), SIZES, (url, size) => {
				const out = artworkUrlAtSize(url, size);
				expect(out).toContain(`-${size}.`);
				// Everything but the token is untouched.
				expect(out.replace(`-${size}.`, '-SIZE.')).toBe(url.replace(/-(t\d+x\d+|original|large|crop)\./, '-SIZE.'));
			}),
			RUNS
		);
	});

	it('is idempotent, and asking for a second size is the same as asking once', () => {
		fc.assert(
			fc.property(artworkUrl(), SIZES, SIZES, (url, a, b) => {
				const once = artworkUrlAtSize(url, a);
				expect(artworkUrlAtSize(once, a)).toBe(once);
				expect(artworkUrlAtSize(once, b)).toBe(artworkUrlAtSize(url, b));
			}),
			RUNS
		);
	});

	it('passes any URL it does not recognise through untouched', () => {
		fc.assert(
			fc.property(bytes(), SIZES, (url, size) => {
				fc.pre(!/-(t\d+x\d+|original|large|crop|badge|small|tiny|mini)\.(jpg|jpeg|png|gif)$/i.test(url));
				expect(artworkUrlAtSize(url, size)).toBe(url);
			}),
			RUNS
		);
	});
});

// ---------------------------------------------------------------------------

describe('download links (links.js)', () => {
	it('reads a host out of anything, or says there is none', () => {
		fc.assert(
			fc.property(fc.oneof(fc.webUrl(), bytes()), (url) => {
				const host = hostOf(url);
				expect(host).toBeTypeOf('string');
				expect(host).toBe(host.toLowerCase());
				expect(host.startsWith('www.')).toBe(false);
			}),
			RUNS
		);
	});

	it('reduces a host to a suffix of itself', () => {
		fc.assert(
			fc.property(fc.domain(), (host) => {
				const domain = domainOf(host);
				expect(host.endsWith(domain)).toBe(true);
				expect(domain.split('.').length).toBeGreaterThanOrEqual(Math.min(2, host.split('.').length));
				expect(domainOf(domain)).toBe(domain);
			}),
			RUNS
		);
	});

	it('unwraps a gate.sc link back to what was wrapped, and leaves the rest alone', () => {
		fc.assert(
			fc.property(fc.webUrl(), (url) => {
				const wrapped = `https://gate.sc?url=${encodeURIComponent(url)}&token=abc`;
				expect(unwrapGate(wrapped)).toBe(url);
				expect(unwrapGateSites(wrapped)).toBe(url);
			}),
			RUNS
		);
	});

	it('leaves a link that is not a gate.sc one untouched', () => {
		fc.assert(
			fc.property(fc.webUrl(), (url) => {
				fc.pre(hostOf(url) !== 'gate.sc');
				expect(unwrapGate(url)).toBe(url);
			}),
			RUNS
		);
	});

	it('never throws unwrapping arbitrary text', () => {
		fc.assert(
			fc.property(bytes(), (text) => {
				expect(unwrapGate(text)).toBeTypeOf('string');
			}),
			RUNS
		);
	});

	const detailsArb = () =>
		fc.option(
			fc.record({
				downloadable: fc.constantFrom(true, false, null),
				hasDownloadsLeft: fc.boolean(),
				purchaseUrl: fc.oneof(fc.constant(''), fc.webUrl(), bytes({ maxLength: 20 })),
				purchaseTitle: fc.oneof(fc.constant(''), words())
			}),
			{ nil: undefined }
		);

	// SAFETY: the functions under test read a listed track's artist, title,
	// permalink, details and domPurchaseTitle, which are what the record makes.
	const detailedTrack = () =>
		fc.record({
			artist: words(),
			title: words(),
			permalink: fc.oneof(fc.constant(''), fc.constant('/user/track'), words()),
			details: detailsArb(),
			domPurchaseTitle: fc.option(words(), { nil: undefined })
		}) as fc.Arbitrary<ListedEntry>;

	it('builds entries that always carry a usable host, and never more than two', () => {
		fc.assert(
			fc.property(detailedTrack(), (track) => {
				const entries = entriesFor(track);
				expect(entries.length).toBeLessThanOrEqual(2);

				for (const entry of entries) {
					expect(entry.host).not.toBe('');
					expect(entry.host.endsWith(entry.domain)).toBe(true);
					expect(entry.label).not.toBe('');
					expect(['soundcloud', 'external']).toContain(entry.kind);
					expect(entry.track).toBe(track);
				}

				// At most one of each kind.
				expect(new Set(entries.map((e) => e.kind)).size).toBe(entries.length);
			}),
			RUNS
		);
	});

	it('leads with one link and trails the rest, best first', () => {
		fc.assert(
			fc.property(detailedTrack(), (track) => {
				const entries = entriesFor(track);
				const { ordered, primary, secondary } = splitEntries(entries);
				// The same entries, split in two with nothing lost or duplicated.
				expect(new Set(ordered)).toEqual(new Set(entries));
				expect(ordered).toHaveLength(entries.length);
				expect(secondary).toEqual(ordered.slice(1));
				expect(primary).toBe(entries.length === 0 ? null : ordered[0]);
			}),
			RUNS
		);
	});

	it('leads with the SoundCloud link where there are both, unless it is spent', () => {
		fc.assert(
			fc.property(detailedTrack(), (track) => {
				const entries = entriesFor(track);
				fc.pre(entries.length === 2);
				const sc = entries.find((e) => e.kind === 'soundcloud');
				expect(splitEntries(entries).primary!.kind).toBe(sc!.spent ? 'external' : 'soundcloud');
			}),
			RUNS
		);
	});

	it('groups every entry exactly once, soundcloud.com first', () => {
		fc.assert(
			fc.property(fc.array(detailedTrack(), { maxLength: 15 }), (tracks) => {
				const all = tracks.flatMap(entriesFor);
				const groups = buildGroups(tracks);
				expect(groups.flatMap(([, entries]) => entries)).toHaveLength(all.length);

				for (const [domain, entries] of groups) {
					for (const entry of entries) expect(entry.domain).toBe(domain);
				}

				// soundcloud.com is first, or nowhere (-1).
				const at = groups.findIndex(([domain]) => domain === 'soundcloud.com');
				expect(at).toBeLessThanOrEqual(0);
				// Ordering is total: busiest first, then alphabetical.
				const rest = groups.slice(at === 0 ? 1 : 0);

				for (let i = 1; i < rest.length; i += 1) {
					const [prevName, prev] = rest[i - 1];
					const [name, entries] = rest[i];
					expect(
						prev.length > entries.length || (prev.length === entries.length && prevName.localeCompare(name) <= 0)
					).toBe(true);
				}
			}),
			RUNS
		);
	});

	it('lists every listed track exactly once, in the order the list holds them', () => {
		fc.assert(
			fc.property(fc.array(detailedTrack(), { maxLength: 15 }), (tracks) => {
				const rows = buildListOrder(tracks);
				// Every one of them, unconditionally: what is on the list is there
				// because somebody put it there, and a row with no answer about it is
				// still that track's row.
				expect(rows.map((r) => r.track)).toEqual(tracks);

				// The three kinds of row are exhaustive over anything the list can
				// hold: entries, known to have none, or not finished being asked
				// about. (A track with no permalink is none of them — and never on
				// the list, which refuses one.)
				const linkless = rows.filter((row) => row.entries.length === 0 && row.track.permalink);

				for (const row of linkless) expect(isWithoutLinks(row.track) || isUnchecked(row.track)).toBe(true);
			}),
			RUNS
		);
	});

	it('marks as pending exactly the unchecked tracks a lookup is running for', () => {
		fc.assert(
			fc.property(fc.array(detailedTrack(), { maxLength: 15 }), (tracks) => {
				const checking = new Set(tracks.map((track) => track.permalink));
				const idle = buildListOrder(tracks);
				const running = buildListOrder(tracks, { checking });
				// Nothing is pending when no lookup is running, however little is
				// known about a track...
				expect(idle.every((row) => !row.pending)).toBe(true);
				// ...and with one running, the pending rows are exactly the tracks
				// nobody has an answer about, still in the list's own order.
				expect(running.filter((row) => row.pending).map((row) => row.track)).toEqual(tracks.filter(isUnchecked));

				for (const row of running.filter((one) => one.pending)) expect(row.entries).toHaveLength(0);
			}),
			RUNS
		);
	});

	it('never calls a track link-less while it still has an entry', () => {
		fc.assert(
			fc.property(detailedTrack(), (track) => {
				fc.pre(isWithoutLinks(track));
				expect(entriesFor(track)).toHaveLength(0);
			}),
			RUNS
		);
	});

	it('notes exactly the tracks it could not check', () => {
		fc.assert(
			fc.property(fc.array(detailedTrack(), { maxLength: 15 }), fc.nat({ max: 5 }), (tracks, lookupFailed) => {
				// SAFETY: lookupNotes reads each track's details and nothing else of it,
				// and the other two fields, which are what this hands it.
				const notes = lookupNotes({ tracks, lookupFailed } as Omit<DetailsAnswer, 'tracks'> & {
					tracks: ListedEntry[];
				});

				const unknown = tracks.filter((t) => !t.details).length;
				expect(notes.includes('couldn’t check')).toBe(unknown > 0);
				// And how many: "1 track", "3 tracks".
				expect(notes.includes(`couldn’t check ${count(unknown, 'track')}`)).toBe(unknown > 0);
			}),
			RUNS
		);
	});
});

// ---------------------------------------------------------------------------

describe('saving files (save.js)', () => {
	it('makes any name into one path segment Chrome accepts', () => {
		fc.assert(
			fc.property(bytes(), (name) => {
				const safe = safeName(name);
				expect(safe).not.toMatch(/[/\\]/);
				expect(safe).not.toMatch(/\p{Cc}/u);
				expect(safe.startsWith('.')).toBe(false);
				expect(safe.length).toBeLessThanOrEqual(180);
			}),
			RUNS
		);
	});

	it('is idempotent, so a name already made safe is not chewed twice', () => {
		fc.assert(
			fc.property(bytes({ maxLength: 60 }), (name) => {
				expect(safeName(safeName(name))).toBe(safeName(name));
			}),
			RUNS
		);
	});

	// A file URL whose last segment carries separators and dots encoded, which
	// are real ones once the segment is decoded into a name. Without these, no
	// URL here ends in a file at all, and every name read out of one is ''.
	const ENCODED = ['a', 'Z', '-', '.', '..', '%2F', '%5C', '%2E', '%20', '%00', '%'];

	const encodedFileUrl = () =>
		fc
			.tuple(
				fc.domain(),
				safePath(),
				fc.array(fc.constantFrom(...ENCODED), { minLength: 1, maxLength: 8 }),
				fc.constantFrom('mp3', 'wav', 'zip')
			)
			.map(([host, path, segment, ext]) => `https://${host}${path}/${segment.join('')}.${ext}`);

	it('never returns a filename that could escape its folder', () => {
		fc.assert(
			fc.property(fc.oneof(encodedFileUrl(), fc.webUrl(), bytes()), (url) => {
				const name = filenameOf(url);
				expect(name).not.toMatch(/[/\\]/);
				expect(name.startsWith('.')).toBe(false);
			}),
			RUNS
		);
	});

	const audioUrl = () =>
		fc
			.tuple(fc.domain(), safePath(), fc.constantFrom('mp3', 'wav', 'flac', 'aiff', 'zip'))
			.map(([host, path, ext]) => `https://${host}${path}.${ext}`);

	it('recognises an audio URL as a file, and takes its name from the path', () => {
		fc.assert(
			fc.property(audioUrl(), (url) => {
				expect(looksLikeFile(url)).toBe(true);
				expect(filenameOf(url)).not.toBe('');
			}),
			RUNS
		);
	});

	it('recovers the filename a signed link carries in its Content-Disposition', () => {
		fc.assert(
			fc.property(words(), fc.constantFrom('mp3', 'wav', 'flac'), (stem, ext) => {
				const wanted = `${stem}.${ext}`;

				const url =
					'https://cdn.example.com/x?response-content-disposition=' +
					encodeURIComponent(`attachment; filename="${wanted}"`);

				expect(filenameOf(url)).toBe(safeName(wanted));
			}),
			RUNS
		);
	});

	it('says a page is not a file', () => {
		fc.assert(
			fc.property(fc.domain(), (host) => {
				expect(looksLikeFile(`https://${host}/some/page`)).toBe(false);
			}),
			RUNS
		);
	});

	it('never throws deciding whether arbitrary text is a file', () => {
		fc.assert(
			fc.property(bytes(), (text) => {
				expect(looksLikeFile(text)).toBeTypeOf('boolean');
			}),
			RUNS
		);
	});
});

// ---------------------------------------------------------------------------

describe('reading other people’s pages (parse.js)', () => {
	const jsonObject = () => fc.dictionary(fc.string({ maxLength: 8 }), fc.jsonValue({ maxDepth: 3 }), { maxKeys: 6 });

	it('finds the object after a marker, whatever is in it or around it', () => {
		const marker = 'window.__STATE__ =';
		fc.assert(
			fc.property(fc.string(), jsonObject(), fc.string(), (before, value, after) => {
				fc.pre(!before.includes(marker));
				const html = `${before}${marker} ${JSON.stringify(value)};${after}`;
				// Compared as JSON: -0 is written "0", and comes back as +0.
				expect(JSON.stringify(jsonAfter(html, marker))).toBe(JSON.stringify(value));
			}),
			RUNS
		);
	});

	it('answers null rather than throwing for anything that is not there', () => {
		fc.assert(
			fc.property(bytes(), bytes({ minLength: 1, maxLength: 8 }), (text, marker) => {
				const out = jsonAfter(text, marker);
				// typeof null is 'object' too: an object, or null.
				expect(out).toBeTypeOf('object');
			}),
			RUNS
		);
	});

	it('rejoins a Next.js flight payload out of its pushed chunks', () => {
		fc.assert(
			fc.property(fc.array(fc.string(), { maxLength: 8 }), fc.string(), (chunks, noise) => {
				fc.pre(!noise.includes('self.__next_f'));

				const html = chunks
					.map((chunk) => `<script>self.__next_f.push([1,${JSON.stringify(chunk)}])</script>`)
					.join(noise);

				expect(flightPayload(html)).toBe(chunks.join(''));
			}),
			RUNS
		);
	});

	it('returns the empty payload for a page that has none', () => {
		fc.assert(
			fc.property(bytes(), (html) => {
				expect(flightPayload(html)).toBeTypeOf('string');
			}),
			RUNS
		);
	});

	const base64url = (text: string) => btoa(text).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

	it('round-trips a base64url payload back into the object it encodes', () => {
		const ascii = fc.string({ unit: 'binary-ascii' }).map((s) => s.replace(/[^\x20-\x7e]/g, ''));
		fc.assert(
			fc.property(fc.dictionary(ascii, ascii, { maxKeys: 5 }), (value) => {
				expect(decodeBase64Json(base64url(JSON.stringify(value)))).toEqual(value);
			}),
			RUNS
		);
	});

	// Near misses as well as noise, each not a payload by construction: text
	// with a character base64 has no place for, the encoding of something no
	// JSON value starts with, and a real payload with its end cut off.
	it('answers null for anything that is not one', () => {
		const ascii = fc.string({ unit: 'binary-ascii' });
		fc.assert(
			fc.property(
				fc.oneof(
					bytes().map((text) => `${text}!`),
					ascii.map((text) => base64url(`x${text}`)),
					fc
						.dictionary(ascii, ascii, { minKeys: 1, maxKeys: 3 })
						.map((value) => base64url(JSON.stringify(value)).slice(0, -2))
				),
				(text) => {
					expect(decodeBase64Json(text)).toBeNull();
				}
			),
			RUNS
		);
	});

	it('parses any markup into a document without running anything', () => {
		fc.assert(
			fc.property(bytes({ maxLength: 200 }), (html) => {
				const doc = documentOf(html);
				expect(doc.body).toBeTruthy();
			}),
			RUNS
		);
	});

	it('reads a form’s named, enabled, non-button fields and nothing else', () => {
		fc.assert(
			fc.property(
				fc.array(
					fc.record({
						name: fc.stringMatching(/^[a-z][a-z0-9_-]{0,8}$/),
						value: words(),
						type: fc.constantFrom('text', 'hidden', 'email', 'submit', 'button', 'checkbox'),
						disabled: fc.boolean(),
						checked: fc.boolean()
					}),
					{ maxLength: 8 }
				),
				(fields) => {
					const form = document.createElement('form');

					const inputs = fields.map((field) => {
						const input = document.createElement('input');
						input.type = field.type;
						input.name = field.name;
						input.value = field.value;
						input.disabled = field.disabled;
						input.checked = field.checked;
						form.append(input);

						return input;
					});

					const wanted = inputs.filter(
						(input, i) =>
							!fields[i].disabled &&
							!['submit', 'button'].includes(fields[i].type) &&
							(fields[i].type !== 'checkbox' || fields[i].checked)
					);

					// Each with the value the field holds (an email field trims what it
					// is given, so that is read back off the field); a name used twice
					// is sent with its last value, as the fields are read in order.
					expect(formFields(form)).toEqual(Object.fromEntries(wanted.map((input) => [input.name, input.value])));
				}
			),
			RUNS
		);
	});
});

// ---------------------------------------------------------------------------

describe('matching links to gate sites (sites/index.js)', () => {
	const claimedUrl = () =>
		fc
			.tuple(fc.constantFrom(...CLAIMED), fc.constantFrom('', 'www.'), safePath())
			.map(([host, www, path]) => `https://${www}${host}${path}`);

	it('matches every host every site claims, with or without www', () => {
		fc.assert(
			fc.property(claimedUrl(), (url) => {
				const site = siteFor(url);
				expect(site).not.toBeNull();
				expect(SITES).toContain(site);
			}),
			RUNS
		);
	});

	it('never matches a scheme a fetch could not follow', () => {
		fc.assert(
			fc.property(
				fc.constantFrom('ftp', 'javascript', 'data', 'file', 'chrome-extension'),
				fc.constantFrom(...CLAIMED),
				(scheme, host) => {
					expect(siteFor(`${scheme}://${host}/x`)).toBeNull();
				}
			),
			RUNS
		);
	});

	it('pulls gate links out of any surrounding prose, and only gate links', () => {
		fc.assert(
			fc.property(
				fc.array(claimedUrl(), { maxLength: 4 }),
				fc.constantFrom(' ', '\n', ', ', ' — ', '\t'),
				(urls, gap) => {
					expect(extractGateUrls(urls.join(gap))).toEqual(urls);
				}
			),
			RUNS
		);
	});

	it('drops the sentence’s punctuation from the end of a link', () => {
		fc.assert(
			fc.property(claimedUrl(), fc.constantFrom('.', ',', ')', '].', '",'), (url, tail) => {
				fc.pre(!/[)\]},.;'"]$/.test(url));
				expect(extractGateUrls(`see ${url}${tail} thanks`)).toEqual([url]);
			}),
			RUNS
		);
	});

	it('finds nothing in text that holds no gate link, and never throws', () => {
		fc.assert(
			fc.property(bytes(), (text) => {
				const found = extractGateUrls(text);
				expect(Array.isArray(found)).toBe(true);
				expect(found.every(isGateUrl)).toBe(true);
			}),
			RUNS
		);
	});

	it('labels any link with something non-empty', () => {
		fc.assert(
			fc.property(fc.oneof(fc.webUrl(), bytes({ minLength: 1 })), (url) => {
				expect(labelOfUrl(url).length).toBeGreaterThan(0);
			}),
			RUNS
		);
	});

	it('fetches only from hosts it claims', () => {
		expect(new Set(HOSTS).size).toBe(HOSTS.length);

		for (const host of HOSTS) expect(CLAIMED).toContain(host);
	});
});
