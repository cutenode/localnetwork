// docs/manager-state.schema.json — the specification of the
// exported state file, checked with a standard JSON Schema validator (Ajv,
// draft 2020-12) rather than with the extension's own code.
//
// The extension ships no dependencies, so state-transfer.ts validates
// imported files by hand. That hand-written check and this schema are two
// statements of one format, and they drift apart silently unless something
// holds them together: the tables below feed the same documents to both and
// require the same verdict from each.
import { describe, it, expect, beforeEach } from 'vitest';
import { loadState, type StateTransfer } from './helpers/load.ts';
import { createValidator, readSchema, explain } from '../scripts/validate-state.mts';
import {
	artistEntry,
	artistProfile,
	file,
	knownDetails,
	listedEntry,
	partialDetails,
	queueEntry,
	type FieldValue
} from './helpers/state-fixtures.ts';

const schema = readSchema();

const validate = createValidator(schema);

// Whether the schema accepts a document, with Ajv's reasons on failure.
const accepts = (doc: FieldValue) => ({ ok: validate(doc), why: explain(validate.errors).join('\n') });

// Whether state-transfer.js accepts it.
const parses = (state: StateTransfer, doc: FieldValue) => {
	try {
		state.parse(JSON.stringify(doc));

		return { ok: true, why: '' };
	} catch (e) {
		return { ok: false, why: e instanceof Error ? e.message : String(e) };
	}
};

let state: StateTransfer;

beforeEach(async () => {
	state = await loadState();
});

describe('the schema itself', () => {
	it('is JSON Schema 2020-12 and compiles in strict mode', () => {
		expect(schema.$schema).toBe('https://json-schema.org/draft/2020-12/schema');
		expect(schema.$id).toMatch(/^https:\/\//);
		// createValidator would have thrown on an unknown keyword or a dangling
		// $ref; reaching here is the assertion.
		expect(validate).toBeTypeOf('function');
	});

	it('describes every member and definition it declares', () => {
		for (const property of Object.values(schema.properties)) {
			expect(property).toHaveProperty('description', expect.any(String));
		}

		for (const def of Object.values(schema.$defs)) expect(def).toHaveProperty('description', expect.any(String));
	});
});

describe('the schema and the extension agree on the format', () => {
	it('on what the file is called and which version it is', () => {
		// Two names, because the tool was renamed and the format was not: the
		// first is what a new file is written under, the rest are read as well.
		expect(schema.properties.format.enum).toEqual(state.READS);
		expect(state.READS[0]).toBe(state.FORMAT);
		expect(schema.properties.version.const).toBe(state.VERSION);
	});

	it('on which options a file may carry, and what each may hold', () => {
		expect(Object.keys(schema.properties.prefs.properties)).toEqual(state.PREFS.map((p) => p.name));
		expect(state.PREFS.find((p) => p.name === 'sort')).toMatchObject({
			values: schema.properties.prefs.properties.sort.enum
		});
	});

	it('on what a permalink path looks like', () => {
		// The same expression the parser uses (state-transfer.js isPermalink).
		expect(schema.$defs.permalink.pattern).toBe('^/[^/?#\\s]+/[^/?#\\s]+$');
	});
});

// Documents both must accept.
const VALID = {
	'a full export': file(),
	// Written before the tool was renamed to Manager. The format never changed,
	// so the file is read as any other: refusing it would throw away the marks,
	// the cached lookups and the half-worked batch it exists to keep.
	'a full export from before the rename': file({ format: 'toolbox.soundcloud-downloads.state' }),
	'the bare minimum': { format: file().format, version: 1 },
	'no exportedAt': (() => {
		const { exportedAt: _drop, ...rest } = file();

		return rest;
	})(),
	'empty maps': file({ downloaded: {}, details: {}, prefs: {} }),
	'a remembered lookup failure': file({ details: { '/a/b': { unavailable: true, at: 1 } } }),
	'details with a null id': file({ details: { '/a/b': knownDetails({ permalink: '/a/b', id: null }) } }),
	'details naming a cover': file({
		details: {
			'/a/b': knownDetails({ permalink: '/a/b', artworkUrl: 'https://i1.sndcdn.com/a.jpg' })
		}
	}),
	'details naming no cover': file({
		details: { '/a/b': knownDetails({ permalink: '/a/b', artworkUrl: '' }) }
	}),
	'partial details with an empty cover': file({
		queue: [queueEntry({ details: partialDetails({ artworkUrl: '' }) })]
	}),
	'details with no id at all': file({
		details: {
			'/a/b': (() => {
				const { id: _drop, ...rest } = knownDetails({ permalink: '/a/b' });

				return rest;
			})()
		}
	}),
	'an empty queue': file({ queue: [] }),
	'a queued track with nothing but a permalink': file({ queue: [{ permalink: '/a/b' }] }),
	'a queued track with no details at all': (() => {
		const { details: _drop, ...rest } = queueEntry();

		return file({ queue: [rest] });
	})(),
	'a queued track with null details': file({ queue: [queueEntry({ details: null })] }),
	'a queued track with a Likes row’s partial details': file({
		queue: [queueEntry({ details: partialDetails() })]
	}),
	'partial details with no id at all': file({
		queue: [
			queueEntry({
				details: (() => {
					const { id: _drop, ...rest } = partialDetails();

					return rest;
				})()
			})
		]
	}),
	'the same track queued twice': file({ queue: [queueEntry(), queueEntry({ title: 'again' })] }),
	'no list at all, as every file written before there was one': (() => {
		const { listed: _drop, ...rest } = file();

		return rest;
	})(),
	'an empty list': file({ listed: [] }),
	'a listed track with nothing but a permalink': file({ listed: [{ permalink: '/a/b' }] }),
	'a listed track with null details': file({ listed: [listedEntry({ details: null })] }),
	'a listed track nobody has liked or reposted either way': file({
		listed: [listedEntry({ liked: null, reposted: null })]
	}),
	'a listed track whose page rendered no genre': file({
		listed: [listedEntry({ genre: null })]
	}),
	'a queued track carrying none of what only a page says': (() => {
		const { genre: _g, liked: _l, reposted: _r, ...rest } = queueEntry();

		return file({ queue: [rest] });
	})(),
	'a listed track the page said was not liked': file({ listed: [listedEntry({ liked: false })] }),
	'a listed track with a Likes row’s partial details': file({
		listed: [listedEntry({ details: partialDetails() })]
	}),
	'a listed track from nowhere in particular': file({ listed: [listedEntry({ from: '' })] }),
	'a listed track added at the epoch': file({ listed: [listedEntry({ at: 0 })] }),
	'no artists at all, as every file written before there was an artists tool': (() => {
		const { artists: _drop, ...rest } = file();

		return rest;
	})(),
	'an empty artists list': file({ artists: [] }),
	'an artist with nothing but a path': file({ artists: [{ permalink: '/a' }] }),
	'an artist nobody has asked about': file({ artists: [artistEntry({ profile: null })] }),
	'an artist with no public profile': file({
		artists: [artistEntry({ profile: { unavailable: true, at: 1 } })]
	}),
	'an artist whose profile is nothing but a time': file({
		artists: [artistEntry({ profile: { at: 1 } })]
	}),
	'a profile with no links': file({ artists: [artistEntry({ profile: artistProfile({ links: [] }) })] }),
	'a profile with nulls where SoundCloud said nothing': file({
		artists: [artistEntry({ profile: artistProfile({ id: null, followers: null, trackCount: null }) })]
	}),
	'a profile link with nothing but an address': file({
		artists: [artistEntry({ profile: artistProfile({ links: [{ url: 'https://x.com/a' }] }) })]
	}),
	'an artist with no tracks at all': file({ artists: [artistEntry({ tracks: [] })] }),
	'an artist from nowhere in particular': file({ artists: [artistEntry({ from: '' })] }),
	'the same artist listed twice': file({ artists: [artistEntry(), artistEntry({ name: 'again' })] }),
	'one option only': file({ prefs: { sort: 'site' } }),
	'at exactly zero': file({ details: { '/a/b': { unavailable: true, at: 0 } } }),
	// RFC 3339 is more than what toISOString writes, and a file may have been
	// written by something else.
	'a mark timed with an offset': file({ downloaded: { '/a/b': '2026-09-08T13:00:00+02:00' } }),
	'a mark timed to the second': file({ downloaded: { '/a/b': '2026-09-08T12:00:00Z' } }),
	'a mark timed in lower case, with a space for the T': file({
		downloaded: { '/a/b': '2026-09-08 12:00:00.5z' }
	}),
	'an exportedAt on a leap day': file({ exportedAt: '2024-02-29T12:00:00.000Z' })
};

// Documents both must refuse, and why.
const INVALID = {
	'not an object': [],
	'no format': { version: 1 },
	'a foreign format': file({ format: 'something.else' }),
	'a version this tool does not read': file({ version: 2 }),
	'a member of its own invention': file({ marks: {} }),
	'an exportedAt that is not a time': file({ exportedAt: 'whenever' }),
	'downloaded that is not a map': file({ downloaded: [] }),
	'a mark under a key that is not a permalink': file({ downloaded: { 'bicep/glue': file().exportedAt } }),
	'a mark under a one-segment path': file({ downloaded: { '/bicep': file().exportedAt } }),
	'a mark under a three-segment path': file({ downloaded: { '/a/b/c': file().exportedAt } }),
	'a mark under a path with a query': file({ downloaded: { '/a/b?in=c': file().exportedAt } }),
	'a mark timed in milliseconds': file({ downloaded: { '/a/b': 1757332800000 } }),
	'a mark timed in nonsense': file({ downloaded: { '/a/b': 'whenever' } }),
	// Every one of these Date.parse reads without complaint.
	'a mark timed as a bare date': file({ downloaded: { '/a/b': '2026-09-08' } }),
	'a mark timed as a lone number': file({ downloaded: { '/a/b': '1' } }),
	'a mark timed in words': file({ downloaded: { '/a/b': 'Sep 8 2026' } }),
	'a mark timed with no seconds': file({ downloaded: { '/a/b': '2026-09-08T12:00Z' } }),
	'a mark timed with no zone': file({ downloaded: { '/a/b': '2026-09-08T12:00:00' } }),
	'a mark on a day its month does not have': file({
		downloaded: { '/a/b': '2026-02-30T12:00:00Z' }
	}),
	'a mark at the 24th hour': file({ downloaded: { '/a/b': '2026-09-08T24:00:00Z' } }),
	'an exportedAt in words': file({ exportedAt: 'Sep 8 2026' }),
	'details that are not a map': file({ details: 'nope' }),
	'a details entry that is not an object': file({ details: { '/a/b': 'nope' } }),
	'details missing downloadable': file({
		details: {
			'/a/b': (() => {
				const { downloadable: _drop, ...rest } = knownDetails({ permalink: '/a/b' });

				return rest;
			})()
		}
	}),
	'details with a non-boolean downloadable': file({
		details: { '/a/b': knownDetails({ permalink: '/a/b', downloadable: 'yes' }) }
	}),
	'details with a non-boolean hasDownloadsLeft': file({
		details: { '/a/b': knownDetails({ permalink: '/a/b', hasDownloadsLeft: 1 }) }
	}),
	'details with a non-string purchaseUrl': file({
		details: { '/a/b': knownDetails({ permalink: '/a/b', purchaseUrl: null }) }
	}),
	'details with a non-string purchaseTitle': file({
		details: { '/a/b': knownDetails({ permalink: '/a/b', purchaseTitle: 7 }) }
	}),
	'details with a fractional at': file({
		details: { '/a/b': knownDetails({ permalink: '/a/b', at: 1.5 }) }
	}),
	'details with a negative at': file({
		details: { '/a/b': knownDetails({ permalink: '/a/b', at: -1 }) }
	}),
	'details with a non-string cover': file({
		details: { '/a/b': knownDetails({ permalink: '/a/b', artworkUrl: 7 }) }
	}),
	'partial details claiming a cover the row never showed': file({
		queue: [queueEntry({ details: partialDetails({ artworkUrl: 'https://i1.sndcdn.com/a.jpg' }) })]
	}),
	'details with a string id': file({
		details: { '/a/b': knownDetails({ permalink: '/a/b', id: 'x' }) }
	}),
	'details with a field of its own invention': file({
		details: { '/a/b': knownDetails({ permalink: '/a/b', extra: true }) }
	}),
	'details with a permalink that is not a path': file({
		details: { '/a/b': knownDetails({ permalink: 'nonsense' }) }
	}),
	'a failure entry that is not a failure': file({ details: { '/a/b': { unavailable: false, at: 1 } } }),
	'a failure entry with no time': file({ details: { '/a/b': { unavailable: true } } }),
	'a failure entry carrying more': file({ details: { '/a/b': { unavailable: true, at: 1, id: 1 } } }),
	'artists that are not an array': file({ artists: {} }),
	'an artist that is not an object': file({ artists: ['/a'] }),
	'an artist with no path': file({ artists: [{ name: 'A' }] }),
	'an artist under a track’s path': file({ artists: [artistEntry({ permalink: '/a/b' })] }),
	'an artist under a path with no slash': file({ artists: [artistEntry({ permalink: 'bicep' })] }),
	'an artist with a non-string name': file({ artists: [artistEntry({ name: 7 })] }),
	'an artist whose tracks are not an array': file({ artists: [artistEntry({ tracks: {} })] }),
	'an artist track that is not an object': file({ artists: [artistEntry({ tracks: ['/a/b'] })] }),
	'an artist track with no permalink': file({ artists: [artistEntry({ tracks: [{ title: 'x' }] })] }),
	'an artist track under an artist’s path': file({
		artists: [artistEntry({ tracks: [{ permalink: '/a' }] })]
	}),
	'an artist track with a field of its own invention': file({
		artists: [artistEntry({ tracks: [{ permalink: '/a/b', extra: 1 }] })]
	}),
	'an artist with a non-string from': file({ artists: [artistEntry({ from: null })] }),
	'an artist added at no time at all': file({ artists: [artistEntry({ at: 'whenever' })] }),
	'an artist with a field of its own invention': file({ artists: [artistEntry({ extra: true })] }),
	'a profile that is not one': file({ artists: [artistEntry({ profile: 'nope' })] }),
	'a profile with no time': file({ artists: [artistEntry({ profile: {} })] }),
	'a profile with a string id': file({ artists: [artistEntry({ profile: artistProfile({ id: 'x' }) })] }),
	'a profile under a track’s path': file({
		artists: [artistEntry({ profile: artistProfile({ permalink: '/a/b' }) })]
	}),
	'a profile with a string follower count': file({
		artists: [artistEntry({ profile: artistProfile({ followers: 'many' }) })]
	}),
	'a profile with a fractional track count': file({
		artists: [artistEntry({ profile: artistProfile({ trackCount: 1.5 }) })]
	}),
	'a profile with a non-boolean verified': file({
		artists: [artistEntry({ profile: artistProfile({ verified: 'yes' }) })]
	}),
	'a profile with a non-string bio': file({ artists: [artistEntry({ profile: artistProfile({ bio: 7 }) })] }),
	'a profile whose links are not an array': file({
		artists: [artistEntry({ profile: artistProfile({ links: 'x' }) })]
	}),
	'a profile link that is not an object': file({
		artists: [artistEntry({ profile: artistProfile({ links: ['x'] }) })]
	}),
	'a profile link with no address': file({
		artists: [artistEntry({ profile: artistProfile({ links: [{ network: 'x' }] }) })]
	}),
	'a profile link with a field of its own invention': file({
		artists: [artistEntry({ profile: artistProfile({ links: [{ url: 'x', extra: 1 }] }) })]
	}),
	'a profile with a field of its own invention': file({
		artists: [artistEntry({ profile: artistProfile({ extra: true }) })]
	}),
	'a failed profile with no time': file({ artists: [artistEntry({ profile: { unavailable: true } })] }),
	'a failed profile carrying more': file({
		artists: [artistEntry({ profile: { unavailable: true, at: 1, id: 1 } })]
	}),
	'a failed profile that is not a failure': file({
		artists: [artistEntry({ profile: { unavailable: false, at: 1 } })]
	}),
	'a list that is not an array': file({ listed: {} }),
	'a listed track that is not an object': file({ listed: ['/a/b'] }),
	'a listed track with no permalink': file({ listed: [{ title: 'Glue' }] }),
	'a listed track with a non-string genre': file({ listed: [listedEntry({ genre: 7 })] }),
	'a queued track with a non-string genre': file({ queue: [queueEntry({ genre: [] })] }),
	'a queued track with a non-boolean liked': file({ queue: [queueEntry({ liked: 'yes' })] }),
	'a listed track with a non-boolean liked': file({ listed: [listedEntry({ liked: 'yes' })] }),
	'a listed track with a non-boolean reposted': file({ listed: [listedEntry({ reposted: 1 })] }),
	'a listed track with a non-string domPurchaseTitle': file({
		listed: [listedEntry({ domPurchaseTitle: 7 })]
	}),
	'a listed track with a non-string from': file({ listed: [listedEntry({ from: null })] }),
	'a listed track added at no time at all': file({ listed: [listedEntry({ at: 'whenever' })] }),
	'a listed track added at a fractional time': file({ listed: [listedEntry({ at: 1.5 })] }),
	'a listed track with a field of its own invention': file({
		listed: [listedEntry({ extra: true })]
	}),
	'a listed track carrying a remembered lookup failure': file({
		listed: [listedEntry({ details: { unavailable: true, at: 1 } })]
	}),
	'a queue that is not an array': file({ queue: {} }),
	'a queued track that is not an object': file({ queue: ['/a/b'] }),
	'a queued track with no permalink': file({ queue: [{ title: 'Glue' }] }),
	'a queued track under a path that is not a permalink': file({
		queue: [queueEntry({ permalink: 'bicep/glue' })]
	}),
	'a queued track with a non-string title': file({ queue: [queueEntry({ title: 7 })] }),
	'a queued track with a non-string artist': file({ queue: [queueEntry({ artist: null })] }),
	'a queued track with a non-string artworkUrl': file({ queue: [queueEntry({ artworkUrl: [] })] }),
	'a queued track with a field of its own invention': file({
		queue: [queueEntry({ extra: true })]
	}),
	'a queued track whose details are not details': file({ queue: [queueEntry({ details: 'nope' })] }),
	'a queued track whose details are missing a field': file({
		queue: [
			queueEntry({
				details: (() => {
					const { at: _drop, ...rest } = knownDetails();

					return rest;
				})()
			})
		]
	}),
	'partial details that settle what they cannot know': file({
		queue: [queueEntry({ details: partialDetails({ downloadable: false }) })]
	}),
	'partial details carrying a time': file({
		queue: [queueEntry({ details: partialDetails({ at: 1 }) })]
	}),
	'partial details with an id': file({ queue: [queueEntry({ details: partialDetails({ id: 42 }) })] }),
	'partial details with no permalink': file({
		queue: [
			queueEntry({
				details: (() => {
					const { permalink: _drop, ...rest } = partialDetails();

					return rest;
				})()
			})
		]
	}),
	'a queued track carrying a remembered lookup failure': file({
		queue: [queueEntry({ details: { unavailable: true, at: 1 } })]
	}),
	'prefs that are not an object': file({ prefs: null }),
	'an option this tool does not have': file({ prefs: { theme: 'dark' } }),
	'a non-boolean background': file({ prefs: { background: 'yes' } }),
	'a non-boolean hideDownloaded': file({ prefs: { hideDownloaded: 1 } }),
	'a sort order that does not exist': file({ prefs: { sort: 'sideways' } })
};

describe('what the schema accepts, the importer accepts', () => {
	it.each(Object.entries(VALID))('%s', (_name, doc) => {
		const bySchema = accepts(doc);
		expect(bySchema.ok, bySchema.why).toBe(true);
		const byParser = parses(state, doc);
		expect(byParser.ok, byParser.why).toBe(true);
	});
});

describe('what the schema refuses, the importer refuses', () => {
	it.each(Object.entries(INVALID))('%s', (_name, doc) => {
		expect(accepts(doc).ok, 'the schema should refuse this').toBe(false);
		expect(parses(state, doc).ok, 'the importer should refuse this').toBe(false);
	});
});

// JSON Schema cannot say "this property must equal that one", so every place
// where a permalink has to agree with another is the importer's alone: the
// schema accepts these documents and state-transfer.js refuses them. The
// asymmetries are deliberate; this is where the schema's half is pinned down,
// so none of them is mistaken for drift. The importer's half — the refusal,
// and the words it is made in — is test/state-transfer.test.ts's, but for the
// list's, which is here.
describe('the checks the schema cannot make', () => {
	// Details filed under one track while naming another. Restored, they would
	// answer for the wrong track ever after.
	const misfiled = file({ details: { '/other/track': knownDetails() } });

	// The same disagreement one level down: the details a track was queued with
	// name a different track, which is the Downloader sent to another track's
	// gate. Both kinds of queued details carry a permalink, so both are checked.
	const misqueued = file({ queue: [queueEntry({ permalink: '/other/track' })] });

	const misqueuedPartial = file({
		queue: [queueEntry({ details: partialDetails({ permalink: '/other/track' }) })]
	});

	// And once more on the list, where the same mismatch would put one track's
	// links on another's row for as long as the list is kept.
	const mislisted = file({ listed: [listedEntry({ permalink: '/other/track' })] });

	it.each([
		['details filed under another track', misfiled],
		['a queued track whose details name another', misqueued],
		['a queued track whose partial details name another', misqueuedPartial],
		['a listed track whose details name another', mislisted]
	])('%s is accepted by the schema', (_name, doc) => {
		expect(accepts(doc).ok).toBe(true);
	});

	it('refuses listed details naming a track other than the one listed', () => {
		const { ok, why } = parses(state, mislisted);
		expect(ok).toBe(false);
		expect(why).toMatch(/says \/bicep\/glue, but the track listed is \/other\/track/);
	});
});

describe('what the panel actually exports', () => {
	it('validates against the schema', async () => {
		globalThis.chrome.storage.local.get = async () => ({
			downloaded: { '/bicep/glue': '2026-09-08T12:00:00.000Z' },
			details: { '/bicep/glue': knownDetails() }
		});
		localStorage.setItem('pref:background', '1');
		localStorage.setItem('pref:sort', 'page');
		localStorage.setItem('pref:hide-downloaded', '0');

		// The queue is handed to `snapshot` by the panel, which takes it from the
		// store that owns it (queue.entries) rather than from session storage.
		const snapshot = await state.snapshot([
			queueEntry(),
			queueEntry({
				permalink: '/other/track',
				details: partialDetails({ permalink: '/other/track' })
			})
		]);

		const result = accepts(snapshot);
		expect(result.ok, result.why).toBe(true);

		// And the serialized form is what a validator would be pointed at.
		const onDisk = JSON.parse(state.serialize(snapshot));
		expect(accepts(onDisk).ok).toBe(true);
	});

	it('validates when there is nothing stored at all', async () => {
		const snapshot = await state.snapshot();
		const result = accepts(snapshot);
		expect(result.ok, result.why).toBe(true);
	});
});
