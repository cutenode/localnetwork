// state-transfer.ts — the import/export of the Manager tool’s saved state.
//
// What a valid file looks like is specified in
// docs/manager-state.schema.json; test/state-schema.test.ts
// checks this parser and that schema accept and refuse the same things.
import { describe, it, expect, beforeEach } from 'vitest';
import { createChrome, type ChromeMock } from './helpers/chrome.ts';
import { loadState, type StateTransfer } from './helpers/load.ts';
import {
	AT,
	ISO,
	artistEntry,
	artistProfile,
	file,
	knownDetails,
	listedEntry,
	partialDetails,
	queueEntry,
	type FieldValue,
	type Overrides
} from './helpers/state-fixtures.ts';

declare const chrome: ChromeMock;

const json = (value: FieldValue) => JSON.stringify(value);

// The message of the StateError `parse` throws for this text.
const refusalOf = (state: StateTransfer, text: string) => {
	try {
		state.parse(text);
	} catch (e) {
		expect(e).toBeInstanceOf(state.StateError);

		return e instanceof Error ? e.message : String(e);
	}

	throw new Error(`expected ${text} to be refused`);
};

// The same, for this document written out as a file.
const refusal = (state: StateTransfer, doc: FieldValue) => refusalOf(state, json(doc));

let state: StateTransfer;

beforeEach(async () => {
	state = await loadState();
});

describe('the format it reads and writes', () => {
	it('names itself, and keeps the details under the content script’s own key', () => {
		expect(state.FORMAT).toBe('toolbox.manager.state');
		expect(state.VERSION).toBe(1);
		expect(state.DETAILS_KEY).toBe('details');
		// Named, but never reached for: the store that owns the list hands it
		// over and takes it back, as the queue's does.
		expect(state.LISTED_KEY).toBe('listed');
		expect(state.ARTISTS_KEY).toBe('artists');
		// The queue's key is not one of them: this file never reaches into the
		// session area, because the store that owns the queue hands it over and
		// takes it back (src/lib/queue.svelte.js).
		// @ts-expect-error -- asking after a key it deliberately does not have
		expect(state.QUEUE_KEY).toBeUndefined();
	});

	it('maps each option in the file to the localStorage key that holds it', () => {
		expect(state.PREFS.map((p) => [p.name, p.key])).toEqual([
			['background', 'pref:background'],
			['sort', 'pref:sort'],
			['hideDownloaded', 'pref:hide-downloaded']
		]);
	});
});

describe('parse', () => {
	it('accepts a file this tool wrote, unchanged', () => {
		expect(state.parse(json(file()))).toEqual(file());
	});

	it('fills in the six optional members, so callers never check for them', () => {
		expect(state.parse(json({ format: state.FORMAT, version: 1 }))).toEqual({
			format: state.FORMAT,
			version: 1,
			downloaded: {},
			details: {},
			listed: [],
			artists: [],
			queue: [],
			prefs: {}
		});
	});

	// Newer still than the list: every file before the artists tool existed
	// has no such member, and reads back with everything else intact.
	it('reads a file exported before there was an artists tool', () => {
		const old = file();
		delete old.artists;
		const doc = state.parse(json(old));
		expect(doc.artists).toEqual([]);
		expect(doc.listed).toEqual(old.listed);
	});

	// The list is newer than the format, so every file exported before it has
	// no such member — and those still have to read back, marks, lookups and
	// half-worked batch and all.
	it('reads a file exported before the tool kept a list', () => {
		const old = file();
		delete old.listed;
		const doc = state.parse(json(old));
		expect(doc.listed).toEqual([]);
		expect(doc.downloaded).toEqual(old.downloaded);
		expect(doc.queue).toEqual(old.queue);
	});

	it('refuses what is not JSON, or not a JSON object', () => {
		expect(refusalOf(state, 'not json at all')).toMatch(/not JSON/);
		expect(refusalOf(state, '[]')).toMatch(/must be a JSON object/);
		expect(refusalOf(state, 'null')).toMatch(/must be a JSON object/);
		expect(refusalOf(state, '"a string"')).toMatch(/must be a JSON object/);
	});

	// The tool used to be called SoundCloud downloads, and every file exported
	// then says so. Only the name changed, so such a file is read exactly as a
	// current one is — refusing it would lose the marks, the cached lookups and
	// the half-worked batch the file was written to keep.
	it('reads a file exported before the rename', () => {
		const old = file({ format: 'toolbox.soundcloud-downloads.state' });
		const doc = state.parse(JSON.stringify(old));
		expect(doc.downloaded).toEqual(old.downloaded);
		expect(doc.details).toEqual(old.details);
		expect(doc.listed).toEqual(old.listed);
		expect(doc.artists).toEqual(old.artists);
		expect(doc.queue).toEqual(old.queue);
		expect(doc.prefs).toEqual(old.prefs);
	});

	// Read, but never written: what goes out carries the current name.
	it('writes the current name, whatever it read', async () => {
		expect(state.FORMAT).toBe('toolbox.manager.state');
		expect(state.READS[0]).toBe(state.FORMAT);
		expect((await state.snapshot()).format).toBe('toolbox.manager.state');
	});

	it('refuses a file that is not one of ours, and says which it is', () => {
		expect(refusal(state, { version: 1 })).toMatch(/doesn’t say what it is/);
		expect(refusal(state, file({ format: 'something.else' }))).toMatch(
			/that’s a “something.else”, not a Manager state file/
		);
	});

	// The panel shows `error.message`, so a refusal has to be a sentence this
	// file chose. A value that refuses to become a string — `{"toString": null}`
	// is valid JSON, and coercing it throws — used to throw a TypeError out of
	// the message itself, and the panel showed "Cannot convert object to
	// primitive value" instead of what was wrong with the file.
	it('refuses a value that will not turn into a string, in its own words', () => {
		const awkward = { toString: null };
		const refused = refusal(state, file({ format: awkward }));
		expect(refused).toMatch(/not a Manager state file/);
		expect(refused).toContain('{"toString":null}');

		expect(refusal(state, file({ downloaded: { '/bicep/glue': awkward } }))).toMatch(/is not a time/);

		// And every one of them is still a StateError, not something from the
		// engine.
		for (const doc of [file({ format: awkward }), file({ downloaded: { '/bicep/glue': awkward } })]) {
			expect(() => state.parse(json(doc))).toThrow(state.StateError);
		}
	});

	it('refuses a version it does not read', () => {
		expect(refusal(state, file({ version: 2 }))).toMatch(/version 2; this reads version 1/);
		expect(refusal(state, file({ version: undefined }))).toMatch(/version undefined/);
	});

	it('refuses members it does not know, so a typo is not silently dropped', () => {
		expect(refusal(state, file({ marks: {} }))).toMatch(/“marks” is not part of this format/);
	});

	it('refuses an exportedAt that is not a time, but does not require one', () => {
		expect(refusal(state, file({ exportedAt: 'whenever' }))).toMatch(/“exportedAt” is not a time/);
		const { exportedAt: _drop, ...rest } = file();
		expect(state.parse(json(rest)).downloaded).toEqual(file().downloaded);
	});

	describe('downloaded', () => {
		const withMarks = (downloaded: FieldValue) => file({ downloaded });

		it('refuses anything but a map of permalink to time', () => {
			expect(refusal(state, withMarks([]))).toMatch(/“downloaded” must be an object/);
			expect(refusal(state, withMarks({ 'bicep/glue': ISO }))).toMatch(/not a permalink path/);
			expect(refusal(state, withMarks({ '/bicep': ISO }))).toMatch(/not a permalink path/);
			expect(refusal(state, withMarks({ '/a/b/c': ISO }))).toMatch(/not a permalink path/);
			expect(refusal(state, withMarks({ '/a/b?in=c': ISO }))).toMatch(/not a permalink path/);
			expect(refusal(state, withMarks({ '/bicep/glue': 'whenever' }))).toMatch(/is not a time/);
			expect(refusal(state, withMarks({ '/bicep/glue': AT }))).toMatch(/is not a time/);
		});

		// What the schema calls a date-time, and not whatever Date.parse will
		// take: that takes a bare date, a lone number, a sentence, a time with no
		// seconds, and a day the month does not have — all of which the schema
		// refuses, and none of which `merge` could put in order against a mark
		// already here.
		it('refuses a time that is not RFC 3339, however readable', () => {
			const loose = ['2026-09-08', '1', 'Sep 8 2026', '2026-09-08T12:00Z', '2026-02-30T12:00:00Z'];

			for (const at of loose) {
				expect(refusal(state, withMarks({ '/bicep/glue': at })), at).toMatch(/is not a time/);
			}

			expect(refusal(state, file({ exportedAt: 'Sep 8 2026' }))).toMatch(/“exportedAt” is not a time/);
		});

		it('takes an RFC 3339 time however it is written', () => {
			const exact = [
				'2026-09-08T13:00:00+02:00',
				'2026-09-08T12:00:00Z',
				'2026-09-08t12:00:00.5z',
				'2026-09-08 12:00:00Z',
				'2024-02-29T12:00:00Z'
			];

			for (const at of exact) {
				expect(state.parse(json(withMarks({ '/bicep/glue': at }))).downloaded, at).toEqual({
					'/bicep/glue': at
				});
			}
		});
	});

	describe('details', () => {
		const withDetails = (details: FieldValue) => file({ details });

		it('accepts a remembered lookup failure', () => {
			const doc = withDetails({ '/a/b': { unavailable: true, at: AT } });
			expect(state.parse(json(doc)).details).toEqual({ '/a/b': { unavailable: true, at: AT } });
		});

		it('refuses anything but a map of permalink to entry', () => {
			expect(refusal(state, withDetails('nope'))).toMatch(/“details” must be an object/);
			expect(refusal(state, withDetails({ '/a': knownDetails() }))).toMatch(/not a permalink path/);
			expect(refusal(state, withDetails({ '/a/b': 'nope' }))).toMatch(/must be an object/);
		});

		it('requires every field of a known entry, of the right type', () => {
			const at = (permalink: string, extra: Overrides) =>
				withDetails({ [permalink]: knownDetails({ permalink, ...extra }) });

			expect(refusal(state, at('/a/b', { downloadable: undefined }))).toMatch(/“downloadable” is missing/);
			expect(refusal(state, at('/a/b', { downloadable: 'yes' }))).toMatch(/“downloadable” must be true or false/);
			expect(refusal(state, at('/a/b', { hasDownloadsLeft: 1 }))).toMatch(/“hasDownloadsLeft” must be true or false/);
			expect(refusal(state, at('/a/b', { purchaseUrl: null }))).toMatch(/“purchaseUrl” must be a string/);
			expect(refusal(state, at('/a/b', { purchaseTitle: 7 }))).toMatch(/“purchaseTitle” must be a string/);
			expect(refusal(state, at('/a/b', { genre: 7 }))).toMatch(/“genre” must be a string/);
			expect(refusal(state, at('/a/b', { artworkUrl: 7 }))).toMatch(/“artworkUrl” must be a string/);
			expect(refusal(state, at('/a/b', { at: 'now' }))).toMatch(/“at” must be a time in milliseconds/);
			expect(refusal(state, at('/a/b', { at: -1 }))).toMatch(/“at” must be a time in milliseconds/);
			expect(refusal(state, at('/a/b', { at: 1.5 }))).toMatch(/“at” must be a time in milliseconds/);
			expect(refusal(state, at('/a/b', { id: 'x' }))).toMatch(/“id” must be a whole number or null/);
			expect(refusal(state, at('/a/b', { extra: true }))).toMatch(/“extra” is not part of this format/);
		});

		// Details cached before the genre was kept have no such key, and a backup
		// taken then still has to read back.
		it('allows details with no genre at all', () => {
			const { genre: _drop, ...noGenre } = knownDetails({ permalink: '/a/b' });
			expect(state.parse(json(withDetails({ '/a/b': noGenre }))).details['/a/b']).toEqual(noGenre);
		});

		// The cover is kept for the same reason as the genre, and arrived the same
		// way: details cached before it have no such key, and a backup taken then
		// still has to read back.
		it('keeps the cover the details named, and allows details naming none', () => {
			const art = 'https://i1.sndcdn.com/artworks-abc-t120x120.jpg';

			for (const artworkUrl of [art, '']) {
				const entry = knownDetails({ permalink: '/a/b', artworkUrl });
				expect(state.parse(json(withDetails({ '/a/b': entry }))).details['/a/b']).toEqual(entry);
			}

			const { artworkUrl: _drop, ...noArtwork } = knownDetails({ permalink: '/a/b', artworkUrl: art });
			expect(state.parse(json(withDetails({ '/a/b': noArtwork }))).details['/a/b']).toEqual(noArtwork);
		});

		it('allows a null id and an absent one', () => {
			const entry = knownDetails({ permalink: '/a/b', id: null });
			expect(state.parse(json(withDetails({ '/a/b': entry }))).details['/a/b'].id).toBe(null);
			const { id: _drop, ...noId } = entry;
			expect(state.parse(json(withDetails({ '/a/b': noId }))).details['/a/b']).toEqual(noId);
		});

		it('refuses an entry filed under a track other than the one it names', () => {
			expect(refusal(state, withDetails({ '/other/track': knownDetails() }))).toMatch(
				/says \/bicep\/glue, but it is filed under \/other\/track/
			);
			expect(refusal(state, withDetails({ '/a/b': knownDetails({ permalink: 'nonsense' }) }))).toMatch(
				/“permalink” must be a permalink path/
			);
			const { permalink: _drop, ...noPermalink } = knownDetails();
			expect(refusal(state, withDetails({ '/a/b': noPermalink }))).toMatch(/“permalink” is missing/);
		});

		it('checks a failure entry too', () => {
			expect(refusal(state, withDetails({ '/a/b': { unavailable: false, at: AT } }))).toMatch(
				/“unavailable” must be true/
			);
			expect(refusal(state, withDetails({ '/a/b': { unavailable: true } }))).toMatch(/“at” is missing/);
			expect(refusal(state, withDetails({ '/a/b': { unavailable: true, at: AT, id: 1 } }))).toMatch(
				/“id” is not part of this format/
			);
		});
	});

	describe('queue', () => {
		const withQueue = (queue: FieldValue) => file({ queue });

		it('accepts a list of queued tracks, in the order the file has them', () => {
			const queue = [queueEntry(), queueEntry({ permalink: '/other/track', details: null })];
			expect(state.parse(json(withQueue(queue))).queue).toEqual(queue);
		});

		it('needs nothing of an entry but the permalink', () => {
			expect(state.parse(json(withQueue([{ permalink: '/a/b' }]))).queue).toEqual([{ permalink: '/a/b' }]);
		});

		it('refuses anything but an array of entries', () => {
			expect(refusal(state, withQueue({}))).toMatch(/“queue” must be an array/);
			expect(refusal(state, withQueue('nope'))).toMatch(/“queue” must be an array/);
			expect(refusal(state, withQueue(['nope']))).toMatch(/queue #1: must be an object/);
			expect(refusal(state, withQueue([null]))).toMatch(/queue #1: must be an object/);
		});

		it('names the entry it refuses by its place in the queue', () => {
			expect(refusal(state, withQueue([queueEntry(), { title: 'no permalink' }]))).toMatch(
				/queue #2: “permalink” is missing/
			);
		});

		it('requires a permalink path, and the right type of everything else', () => {
			expect(refusal(state, withQueue([{ permalink: 'bicep/glue' }]))).toMatch(/“permalink” must be a permalink path/);
			expect(refusal(state, withQueue([queueEntry({ title: 7 })]))).toMatch(/“title” must be a string/);
			expect(refusal(state, withQueue([queueEntry({ artist: null })]))).toMatch(/“artist” must be a string/);
			expect(refusal(state, withQueue([queueEntry({ artworkUrl: [] })]))).toMatch(/“artworkUrl” must be a string/);
			expect(refusal(state, withQueue([queueEntry({ extra: true })]))).toMatch(/“extra” is not part of this format/);
		});

		it('accepts the details a track was queued with, of either kind, or none', () => {
			const kinds = [knownDetails(), partialDetails(), null];

			for (const details of kinds) {
				expect(state.parse(json(withQueue([queueEntry({ details })]))).queue[0].details).toEqual(details);
			}

			const { details: _drop, ...noDetails } = queueEntry();
			expect(state.parse(json(withQueue([noDetails]))).queue).toEqual([noDetails]);
		});

		it('checks those details as closely as the cache’s own', () => {
			expect(refusal(state, withQueue([queueEntry({ details: 'nope' })]))).toMatch(
				/“details” must be download details or null/
			);
			expect(refusal(state, withQueue([queueEntry({ details: knownDetails({ at: undefined }) })]))).toMatch(
				/queue #1 details: “at” is missing/
			);
			expect(refusal(state, withQueue([queueEntry({ details: knownDetails({ downloadable: 'yes' }) })]))).toMatch(
				/queue #1 details: “downloadable” must be true or false/
			);
		});

		it('takes partial details only with every unknown left unknown', () => {
			expect(refusal(state, withQueue([queueEntry({ details: partialDetails({ downloadable: false }) })]))).toMatch(
				/queue #1 details: “downloadable” must be null/
			);
			expect(refusal(state, withQueue([queueEntry({ details: partialDetails({ hasDownloadsLeft: true }) })]))).toMatch(
				/queue #1 details: “hasDownloadsLeft” must be null/
			);
			expect(refusal(state, withQueue([queueEntry({ details: partialDetails({ id: 42 }) })]))).toMatch(
				/queue #1 details: “id” must be null/
			);
			expect(refusal(state, withQueue([queueEntry({ details: partialDetails({ at: AT }) })]))).toMatch(
				/queue #1 details: “at” is not part of this format/
			);
			expect(refusal(state, withQueue([queueEntry({ details: partialDetails({ partial: false }) })]))).toMatch(
				/queue #1 details: “permalink” is missing|“partial” must be true/
			);
		});

		// The cover is the one unknown the row does answer: it was read off the
		// same row the entry was, so '' is all there is to say and a URL there
		// would be a claim the row never made.
		it('takes a partial cover only as the empty string, or not at all', () => {
			const details = partialDetails({ artworkUrl: '' });
			expect(state.parse(json(withQueue([queueEntry({ details })]))).queue[0].details).toEqual(details);
			expect(
				refusal(
					state,
					withQueue([
						queueEntry({
							details: partialDetails({ artworkUrl: 'https://i1.sndcdn.com/artworks-abc.jpg' })
						})
					])
				)
			).toMatch(/queue #1 details: “artworkUrl” must be an empty string/);
		});

		// The same hazard as details filed under the wrong track, one level down:
		// details naming another track would send the Downloader to that track's
		// gate. Both kinds carry a permalink, so both are checked.
		it('refuses details naming a track other than the one queued', () => {
			expect(refusal(state, withQueue([queueEntry({ permalink: '/other/track' })]))).toMatch(
				/queue #1 details: “permalink” says \/bicep\/glue, but the track queued is \/other\/track/
			);
			expect(
				refusal(state, withQueue([queueEntry({ details: partialDetails({ permalink: '/other/track' }) })]))
			).toMatch(/queue #1 details: “permalink” says \/other\/track, but the track queued is \/bicep\/glue/);
			// The entry it names is the entry it is on, wherever in the queue it is.
			expect(refusal(state, withQueue([queueEntry({ details: null }), queueEntry({ permalink: '/c/d' })]))).toMatch(
				/queue #2 details: “permalink” says \/bicep\/glue, but the track queued is \/c\/d/
			);
		});

		// A lookup that failed is not an answer about the track, so it is never
		// what a track was queued with — and a file saying otherwise is a file
		// that would hand the Downloader something it cannot read.
		it('refuses a remembered lookup failure in place of a track’s details', () => {
			expect(refusal(state, withQueue([queueEntry({ details: { unavailable: true, at: AT } })]))).toMatch(
				/queue #1 details: a remembered lookup failure is not what a track is queued with/
			);
		});
	});

	describe('prefs', () => {
		const withPrefs = (prefs: FieldValue) => file({ prefs });

		it('refuses anything but the options this tool has', () => {
			expect(refusal(state, withPrefs(null))).toMatch(/“prefs” must be an object/);
			expect(refusal(state, withPrefs({ theme: 'dark' }))).toMatch(/“theme” is not an option of this tool/);
			expect(refusal(state, withPrefs({ background: 'yes' }))).toMatch(/“background” must be true or false/);
			expect(refusal(state, withPrefs({ hideDownloaded: 1 }))).toMatch(/“hideDownloaded” must be true or false/);
			expect(refusal(state, withPrefs({ sort: 'sideways' }))).toMatch(/“sort” must be “site” or “page”/);
		});

		it('accepts any subset of them', () => {
			expect(state.parse(json(withPrefs({}))).prefs).toEqual({});
			expect(state.parse(json(withPrefs({ sort: 'site' }))).prefs).toEqual({ sort: 'site' });
		});
	});
});

describe('snapshot', () => {
	// The queued track it is given, whose details name the track it is on, as
	// every entry the queue keeps does.
	const queued = queueEntry({ permalink: '/a/b', details: knownDetails({ permalink: '/a/b' }) });

	// A listed track it is given, whose details name the track it is on.
	const held = listedEntry({ permalink: '/a/b', details: knownDetails({ permalink: '/a/b' }) });

	it('takes both storage keys and the options as they are, and the queue and list from its caller', async () => {
		globalThis.chrome = createChrome({
			storage: { downloaded: { '/a/b': ISO }, details: { '/a/b': knownDetails({ permalink: '/a/b' }) } }
		});
		localStorage.setItem('pref:background', '0');
		localStorage.setItem('pref:sort', 'page');
		localStorage.setItem('pref:hide-downloaded', '1');

		expect(await state.snapshot([queued], [held], [artistEntry()])).toEqual({
			format: state.FORMAT,
			version: 1,
			exportedAt: ISO,
			downloaded: { '/a/b': ISO },
			details: { '/a/b': knownDetails({ permalink: '/a/b' }) },
			listed: [held],
			artists: [artistEntry()],
			queue: [queued],
			prefs: { background: false, sort: 'page', hideDownloaded: true }
		});
	});

	it('is empty, not broken, when nothing has been stored yet', async () => {
		expect(await state.snapshot()).toEqual({
			format: state.FORMAT,
			version: 1,
			exportedAt: ISO,
			downloaded: {},
			details: {},
			listed: [],
			artists: [],
			queue: [],
			prefs: {}
		});
	});

	// The artists, like the list, come from their store and never from storage.
	it('carries the artists it is handed, and reads none from storage', async () => {
		globalThis.chrome = createChrome({
			storage: { artists: [artistEntry({ permalink: '/in-storage' })] }
		});
		expect((await state.snapshot([], [], [artistEntry()])).artists).toEqual([artistEntry()]);
		expect(chrome.storage.local.get).toHaveBeenCalledWith(['downloaded', 'details']);
	});

	// The list, like the queue, is handed in rather than read out of storage:
	// its store shows it on screen even when storage will not take it, and a
	// backup that leaves out what is on screen is no backup.
	it('carries the list it is handed, and reads none from storage', async () => {
		globalThis.chrome = createChrome({
			storage: { listed: [listedEntry({ permalink: '/in/storage' })] }
		});
		expect((await state.snapshot([], [held])).listed).toEqual([held]);
		// What it does read from storage is the marks and the details, together.
		expect(chrome.storage.local.get).toHaveBeenCalledWith(['downloaded', 'details']);
	});

	// The queue on screen is the queue in the file. Its store keeps it in
	// memory when the session area will not take it, so a file written from
	// storage would leave out exactly the batch someone is looking at — and a
	// backup that omits what is on screen is the one thing a backup must not do.
	it('carries the queue it is handed, and reads none from the session area', async () => {
		globalThis.chrome = createChrome({
			session: { queue: [queueEntry({ permalink: '/in/storage', details: null })] }
		});
		expect((await state.snapshot([queued])).queue).toEqual([queued]);
		expect(chrome.storage.session.get).not.toHaveBeenCalled();

		// Nor is a session area needed at all: this browser may have none.
		globalThis.chrome = createChrome();
		// @ts-expect-error -- taking away an area Chrome always has, as this browser may not
		delete chrome.storage.session;
		expect((await state.snapshot([queued])).queue).toEqual([queued]);
	});

	it('skips an option holding something it does not recognise', async () => {
		localStorage.setItem('pref:sort', 'sideways');
		expect((await state.snapshot()).prefs).toEqual({});
	});

	it('carries no options when localStorage is unavailable', async () => {
		const original = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
		Object.defineProperty(globalThis, 'localStorage', {
			configurable: true,
			get() {
				throw new Error('denied');
			}
		});

		try {
			expect((await state.snapshot()).prefs).toEqual({});
		} finally {
			Object.defineProperty(globalThis, 'localStorage', original!);
		}
	});
});

describe('serialize and filename', () => {
	it('writes indented JSON that parses back to the same state', () => {
		const text = state.serialize(file());
		expect(text).toContain('\n  "format"');
		expect(state.parse(text)).toEqual(file());
	});

	it('names the file after the day it was written', () => {
		expect(state.filename()).toBe('manager-2026-09-08.json');
		expect(state.filename(new Date('2024-02-29T23:00:00Z'))).toBe('manager-2024-02-29.json');
	});
});

describe('apply', () => {
	it('adds marks and details to an empty store, and reports what it did', async () => {
		const result = await state.apply(state.parse(json(file())));
		expect(result).toEqual({ marks: 1, details: 1, prefs: 3 });
		expect(chrome.testStore()).toEqual({
			downloaded: { '/bicep/glue': ISO },
			details: { '/bicep/glue': knownDetails() }
		});
		expect(localStorage.getItem('pref:background')).toBe('1');
		expect(localStorage.getItem('pref:sort')).toBe('page');
		expect(localStorage.getItem('pref:hide-downloaded')).toBe('0');
	});

	it('takes details only where they are newer than what is known', async () => {
		const stored = knownDetails({ purchaseTitle: 'already here', at: AT });
		globalThis.chrome = createChrome({ storage: { details: { '/bicep/glue': stored } } });

		const older = file({ details: { '/bicep/glue': knownDetails({ at: AT - 1 }) } });
		expect((await state.apply(state.parse(json(older)))).details).toBe(0);
		expect(chrome.testStore().details['/bicep/glue']).toEqual(stored);

		const same = file({ details: { '/bicep/glue': knownDetails({ at: AT }) } });
		expect((await state.apply(state.parse(json(same)))).details).toBe(0);
		expect(chrome.testStore().details['/bicep/glue']).toEqual(stored);
		// Nothing taken, so nothing written back: the cache is not rewritten for
		// an import that changed none of it.
		const wrote = chrome.storage.local.set.mock.calls.map(([items]) => Object.keys(items));
		expect(wrote.flat()).not.toContain('details');

		const newer = file({ details: { '/bicep/glue': knownDetails({ at: AT + 1 }) } });
		expect((await state.apply(state.parse(json(newer)))).details).toBe(1);
		expect(chrome.testStore().details['/bicep/glue'].at).toBe(AT + 1);
	});

	it('keeps details of tracks the file says nothing about', async () => {
		globalThis.chrome = createChrome({
			storage: { details: { '/keep/me': { unavailable: true, at: AT } } }
		});
		await state.apply(state.parse(json(file())));
		expect(Object.keys(chrome.testStore().details).sort()).toEqual(['/bicep/glue', '/keep/me']);
	});

	it('writes nothing for a file with no data in it', async () => {
		const empty = state.parse(json({ format: state.FORMAT, version: 1 }));
		expect(await state.apply(empty)).toEqual({ marks: 0, details: 0, prefs: 0 });
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});

	// The queue is not this file's to write, however much of one the document
	// carries: the panel hands `doc.queue` to the store that owns it, which is
	// what keeps a track from being queued twice (test/lib/queue.test.ts) and
	// what holds the queue at all when the session area will not take it.
	it('leaves the session queue alone, and never so much as reads it', async () => {
		const held = queueEntry({ permalink: '/already/here', details: null });
		globalThis.chrome = createChrome({ session: { queue: [held] } });

		const doc = file({ queue: [queueEntry(), queueEntry({ permalink: '/c/d', details: null })] });
		expect(await state.apply(state.parse(json(doc)))).toEqual({ marks: 1, details: 1, prefs: 3 });
		expect(chrome.testStore('session').queue).toEqual([held]);
		expect(chrome.storage.session.get).not.toHaveBeenCalled();
		expect(chrome.storage.session.set).not.toHaveBeenCalled();
	});

	// Additive means additive about the options too: one somebody has already
	// chosen here is a thing they have, and a file that disagrees does not get
	// to overrule it. A file is still how an option nobody has touched arrives.
	it('leaves an option that is already set, and sets one that is not', async () => {
		localStorage.setItem('pref:sort', 'site');
		expect((await state.apply(state.parse(json(file())))).prefs).toBe(2);
		expect(localStorage.getItem('pref:sort')).toBe('site');
		expect(localStorage.getItem('pref:background')).toBe('1');
		expect(localStorage.getItem('pref:hide-downloaded')).toBe('0');
	});

	it('leaves the options alone when localStorage is unavailable', async () => {
		const original = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
		Object.defineProperty(globalThis, 'localStorage', {
			configurable: true,
			get() {
				throw new Error('denied');
			}
		});

		try {
			expect((await state.apply(state.parse(json(file())))).prefs).toBe(0);
		} finally {
			Object.defineProperty(globalThis, 'localStorage', original!);
		}
	});
});

// The opt-in, which the sheet makes somebody turn a switch on to reach: every
// part of an import is the other way round, and what is here and not in the
// file is gone.
describe('apply, asked to replace', () => {
	const replacing = (doc: FieldValue) => state.apply(state.parse(json(doc)), { replace: true });

	it('makes the marks and the details cache exactly the file’s', async () => {
		globalThis.chrome = createChrome({
			storage: {
				downloaded: { '/gone/soon': ISO },
				details: { '/gone/soon': { unavailable: true, at: AT } }
			}
		});

		expect(await replacing(file())).toEqual({ marks: 1, details: 1, prefs: 3 });
		expect(chrome.testStore()).toEqual({
			downloaded: { '/bicep/glue': ISO },
			details: { '/bicep/glue': knownDetails() }
		});
	});

	// The one case an add would have refused outright: a file whose details are
	// older than what is cached. Replacing takes them anyway — the file is the
	// answer, not a second opinion.
	it('takes the file’s details even where what is stored is newer', async () => {
		globalThis.chrome = createChrome({
			storage: { details: { '/bicep/glue': knownDetails({ at: AT + 1000 }) } }
		});
		const older = file({ details: { '/bicep/glue': knownDetails({ at: AT }) } });
		expect((await replacing(older)).details).toBe(1);
		expect(chrome.testStore().details['/bicep/glue'].at).toBe(AT);
	});

	it('empties both for a file that carries neither', async () => {
		globalThis.chrome = createChrome({
			storage: { downloaded: { '/gone/soon': ISO }, details: { '/gone/soon': knownDetails() } }
		});
		const bare = { format: state.FORMAT, version: 1 };
		expect(await replacing(bare)).toEqual({ marks: 0, details: 0, prefs: 0 });
		expect(chrome.testStore()).toEqual({ downloaded: {}, details: {} });
	});

	it('overrules an option that is already set', async () => {
		localStorage.setItem('pref:sort', 'site');
		expect((await replacing(file())).prefs).toBe(3);
		expect(localStorage.getItem('pref:sort')).toBe('page');
	});

	// Replacing means the whole set of options, not the ones the file happens to
	// name: an option the file is silent about goes back to its default, which
	// for these is nothing stored at all.
	it('puts an option the file says nothing about back to its default', async () => {
		localStorage.setItem('pref:sort', 'site');
		localStorage.setItem('pref:background', '1');
		const quiet = file({ prefs: { hideDownloaded: true } });

		expect((await replacing(quiet)).prefs).toBe(3);
		expect(localStorage.getItem('pref:hide-downloaded')).toBe('1');
		expect(localStorage.getItem('pref:sort')).toBe(null);
		expect(localStorage.getItem('pref:background')).toBe(null);
	});

	it('counts no option it did not have to touch', async () => {
		const quiet = file({ prefs: {} });
		expect((await replacing(quiet)).prefs).toBe(0);
	});

	// Same as an add: the queue and the list are the stores' to write, and this
	// never so much as looks at the session area.
	it('still leaves the session queue to the store that owns it', async () => {
		const held = queueEntry({ permalink: '/already/here', details: null });
		globalThis.chrome = createChrome({ session: { queue: [held] } });
		await replacing(file());
		expect(chrome.testStore('session').queue).toEqual([held]);
		expect(chrome.storage.session.set).not.toHaveBeenCalled();
	});
});

describe('parse, on the artists', () => {
	const withArtists = (artists: FieldValue) => file({ artists });
	const artist = (extra: Overrides) => withArtists([artistEntry(extra)]);
	const profile = (extra: Overrides) => artist({ profile: artistProfile(extra) });

	it('accepts an artist nobody has asked about, and one with no public profile', () => {
		expect(state.parse(json(artist({ profile: null }))).artists[0].profile).toBe(null);
		const failed = { unavailable: true, at: AT };
		expect(state.parse(json(artist({ profile: failed }))).artists[0].profile).toEqual(failed);
	});

	it('accepts an artist with nothing but a path, and a profile with nothing but a time', () => {
		expect(state.parse(json(withArtists([{ permalink: '/a' }]))).artists).toEqual([{ permalink: '/a' }]);
		expect(state.parse(json(artist({ profile: { at: AT } }))).artists[0].profile).toEqual({
			at: AT
		});
	});

	it('refuses anything but an array of artists', () => {
		expect(refusal(state, withArtists({}))).toMatch(/“artists” must be an array/);
		expect(refusal(state, withArtists(['/a']))).toMatch(/artists #1: must be an object/);
		expect(refusal(state, withArtists([{ name: 'A' }]))).toMatch(/“permalink” is missing/);
		expect(refusal(state, artist({ permalink: '/a/b' }))).toMatch(/must be an artist’s path/);
		expect(refusal(state, artist({ permalink: 'a' }))).toMatch(/must be an artist’s path/);
		expect(refusal(state, artist({ name: 7 }))).toMatch(/“name” must be a string/);
		expect(refusal(state, artist({ from: null }))).toMatch(/“from” must be a string/);
		expect(refusal(state, artist({ at: 'now' }))).toMatch(/“at” must be a time/);
		expect(refusal(state, artist({ extra: true }))).toMatch(/“extra” is not part of this format/);
	});

	it('refuses tracks that are not tracks', () => {
		expect(refusal(state, artist({ tracks: {} }))).toMatch(/“tracks” must be an array/);
		expect(refusal(state, artist({ tracks: ['/a/b'] }))).toMatch(/track #1: must be an object/);
		expect(refusal(state, artist({ tracks: [{ title: 'x' }] }))).toMatch(/“permalink” is missing/);
		expect(refusal(state, artist({ tracks: [{ permalink: '/a' }] }))).toMatch(/must be a permalink path/);
		expect(refusal(state, artist({ tracks: [{ permalink: '/a/b', title: 1 }] }))).toMatch(/“title” must be a string/);
	});

	it('refuses a profile that is not one', () => {
		expect(refusal(state, artist({ profile: 'nope' }))).toMatch(/“profile” must be a profile or null/);
		expect(refusal(state, artist({ profile: {} }))).toMatch(/profile: “at” is missing/);
		expect(refusal(state, profile({ id: 'x' }))).toMatch(/“id” must be a whole number or null/);
		expect(refusal(state, profile({ permalink: '/a/b' }))).toMatch(/must be an artist’s path/);
		expect(refusal(state, profile({ followers: 'many' }))).toMatch(/“followers” must be/);
		expect(refusal(state, profile({ trackCount: 1.5 }))).toMatch(/“trackCount” must be/);
		expect(refusal(state, profile({ verified: 'yes' }))).toMatch(/“verified” must be true or false/);

		for (const field of ['username', 'fullName', 'city', 'country', 'avatarUrl', 'bio']) {
			expect(refusal(state, profile({ [field]: 7 }))).toMatch(new RegExp(`“${field}” must be a string`));
		}

		expect(refusal(state, profile({ links: 'x' }))).toMatch(/“links” must be an array/);
		expect(refusal(state, profile({ links: ['x'] }))).toMatch(/link #1: must be an object/);
		expect(refusal(state, profile({ links: [{ network: 'x' }] }))).toMatch(/“url” is missing/);
		expect(refusal(state, profile({ links: [{ url: 'x', extra: 1 }] }))).toMatch(/“extra” is not part of this format/);
		expect(refusal(state, profile({ links: [{ url: 'x', title: 1 }] }))).toMatch(/“title” must be a string/);
		expect(refusal(state, profile({ extra: true }))).toMatch(/“extra” is not part of this format/);
	});

	it('refuses a remembered failure that carries more than the failure', () => {
		expect(refusal(state, artist({ profile: { unavailable: true } }))).toMatch(/“at” is missing/);
		expect(refusal(state, artist({ profile: { unavailable: true, at: AT, id: 1 } }))).toMatch(
			/“id” is not part of this format/
		);
		expect(refusal(state, artist({ profile: { unavailable: false, at: AT } }))).toMatch(/“unavailable” must be true/);
	});
});

describe('summary', () => {
	it('counts what a parsed file holds, and says when it was written', () => {
		expect(state.summary(state.parse(json(file())))).toEqual({
			marks: 1,
			details: 1,
			listed: 1,
			artists: 1,
			queue: 1,
			prefs: 3,
			exportedAt: ISO
		});
	});

	it('counts zeroes, and no time at all, for a file carrying nothing', () => {
		const bare = state.parse(json({ format: state.FORMAT, version: 1 }));
		expect(state.summary(bare)).toEqual({
			marks: 0,
			details: 0,
			listed: 0,
			artists: 0,
			queue: 0,
			prefs: 0,
			exportedAt: ''
		});
	});
});
