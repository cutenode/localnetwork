// Import and export of everything the extension holds, as one JSON file: the
// Manager's own list of tracks, the ones marked as downloaded, the cached
// download details, the artists tool's list, the session queue and the
// panels' remembered options.
//
// Why it exists: the list, the marks and the details cache live in the
// extension's own storage, which goes away with the extension. Reinstalling it
// (or moving to another browser profile) otherwise means losing a list built
// over months, every mark on it, and paying for every API lookup again. The
// queue goes sooner than any of them — it is a working list, and closing the
// browser is enough to end it — so a file is also the only way a batch picked
// out but not yet worked through outlives the day it was picked. Exporting
// writes a file; importing merges it back.
//
// The file's shape is specified in docs/manager-state.schema.json
// (JSON Schema 2020-12), and `parse` below enforces exactly that schema by
// hand — the extension ships no dependencies, so it cannot run a validator at
// runtime. The two are held together by test/state-schema.test.ts, which
// checks every case against the real schema with Ajv; keep them in step.
//
// Loaded by the popup page, which uses the TbxState global it defines; the
// section that drives it is in the settings sheet
// (src/lib/components/StateTransfer.svelte), not in any one tool. What this
// file owns is the format; the stores own the storage. The marks go through
// TbxDownloaded, and the session queue and the Manager's list through the
// popup's own stores — the sheet hands both in on the way out and hands the
// file's back to those stores on the way in, so what is exported is what the
// popup is showing and what is imported goes through the thing that already
// knows not to hold a track twice.
//
// Importing adds. Every part of it does, and `apply` says how; replacing is
// an opt-in the sheet makes you turn on, and then every one of those parts is
// the other way round.

const TbxState = (() => {
	type StateFile = Tbx.StateFile;

	type ParsedState = Tbx.ParsedState;

	type StatePrefs = Tbx.StatePrefs;

	type QueueEntry = Tbx.QueueEntry;

	type ListedEntry = Tbx.ListedEntry;

	type ArtistEntry = Tbx.ArtistEntry;

	type LocalArea = Tbx.LocalArea;

	type Json = Tbx.Json;

	type JsonObject = Tbx.JsonObject;

	/** What a chrome.storage.local get answers. */
	type Stored = Partial<LocalArea>;

	/** One panel option, as the file and localStorage each name it. */
	type Pref =
		| { name: keyof StatePrefs; key: string; type: 'boolean' }
		| {
				name: keyof StatePrefs;
				key: string;
				type: 'enum';
				/** An enum's allowed values. */
				values: string[];
		  };

	/** What checkMembers asks of one property. */
	interface Rule {
		required?: boolean;
		check: (value: Json) => boolean;
		/** What passes `check`, as a refusal words it. */
		expected: string;
	}

	const FORMAT = 'toolbox.manager.state' as const;

	// What the tool was called before, and so what every file exported before
	// the rename says it is. The format never changed — only its name did — so
	// such a file is read exactly as a current one is, and rewritten under the
	// current name the next time one is exported. Refusing it would lose the
	// marks, the cached lookups and the half-worked batch it was written to
	// keep, which is the whole point of the file.
	const READS: StateFile['format'][] = [FORMAT, 'toolbox.soundcloud-downloads.state'];

	const VERSION = 1 as const;

	// The details cache belongs to the content script (content.js, DETAILS_KEY);
	// this is the same key, read and written here so a restored file brings the
	// lookups back. test/consistency.test.ts checks the two agree.
	const DETAILS_KEY = 'details' as const;

	// The Manager's own list of tracks, which belongs to the popup's listed
	// store (src/tools/manager/listed.svelte.js, LISTED_KEY). Named here only so
	// the file's member and the storage key are known to be the same word;
	// nothing in this file reads or writes it, for the reason the queue is not
	// read or written here either — the store that owns a thing does that.
	const LISTED_KEY = 'listed' as const;

	// The artists tool's own list, which belongs to its store
	// (src/tools/artists/artists.svelte.js, ARTISTS_KEY). Named here for the
	// reason the Manager's list is, and read and written here as little.
	const ARTISTS_KEY = 'artists' as const;

	// The panel options, which live in the extension pages' own localStorage:
	// the name in the file, the localStorage key, and what a valid value is.
	// `background` and `sort` are the Manager panel's (panel.js); the
	// "Hide downloaded" toggle is the shared filter bar's (list-filter.js).
	const PREFS: Pref[] = [
		{ name: 'background', key: 'pref:background', type: 'boolean' },
		{ name: 'sort', key: 'pref:sort', type: 'enum', values: ['site', 'page'] },
		{ name: 'hideDownloaded', key: 'pref:hide-downloaded', type: 'boolean' }
	];

	// --- reading the schema's vocabulary ---------------------------------------

	// JSON's own kinds, which every check below is built from.
	const isString = (value: Json): value is string => typeof value === 'string';
	const isBoolean = (value: Json): value is boolean => typeof value === 'boolean';

	const isObject = (value: Json): value is JsonObject =>
		typeof value === 'object' && value !== null && !Array.isArray(value);

	const isIntegerOrNull = (value: Json) => value === null || Number.isInteger(value);

	// A track's permalink path, the schema's `permalink`: "/uploader/track".
	const isPermalink = (value: Json) => isString(value) && /^\/[^/?#\s]+\/[^/?#\s]+$/.test(value);

	// An artist's path, the schema's `artistPath`: "/uploader".
	const isArtistPath = (value: Json) => isString(value) && /^\/[^/?#\s]+$/.test(value);

	// The schema's `timestamp`: whole milliseconds since the epoch. (Number() of
	// a number is that number.)
	const isTimestamp = (value: Json) => Number.isInteger(value) && Number(value) >= 0;

	// The schema's date-time format: RFC 3339, which is what
	// Date.prototype.toISOString writes — a date, a time to the second, and a
	// zone, "Z" or an offset — with the letters in either case and a space
	// allowed for the "T", as the schema's validator reads it. Date.parse alone
	// takes far more than that (a bare date, a lone number, "Sep 8 2026"), and a
	// mark timed like that is one the schema refuses and TbxDownloaded.merge
	// cannot put in order. Two things the validator takes that this does not —
	// a leap second, and an offset written without its colon — are nothing
	// this tool ever writes.
	const DATE_TIME = new RegExp(
		'^(\\d{4})-(\\d{2})-(\\d{2})[Tt ]' +
			'(?:[01]\\d|2[0-3]):[0-5]\\d:[0-5]\\d(?:\\.\\d+)?' +
			'(?:[Zz]|[+-](?:[01]\\d|2[0-3]):[0-5]\\d)$'
	);

	const isDateTime = (value: Json) => {
		const parts = isString(value) && DATE_TIME.exec(value);

		if (!parts) return false;
		// Date rolls a day the month does not have (the 30th of February) over
		// into the next month without a word, and the schema refuses one; so the
		// day a time names has to be the day it comes out as.
		const [year, month, day] = parts.slice(1).map(Number);
		const date = new Date(Date.UTC(year, month - 1, day));

		return date.getUTCMonth() === month - 1 && date.getUTCDate() === day;
	};

	class StateError extends Error {}

	// Typed on the binding rather than the arrow, so a call narrows what follows it.
	const fail: (message: string) => never = (message) => {
		throw new StateError(message);
	};

	// A value out of the file, put into a refusal's message.
	//
	// Interpolating one straight into a template calls its own `toString`, and
	// a file is free to carry `{"toString": null}` — an object JSON.parse is
	// happy with and string coercion is not. That throws a TypeError out of the
	// very sentence meant to explain the refusal, so the panel shows "Cannot
	// convert object to primitive value" instead of what is wrong with the
	// file. Strings go through as they are, so every message a real file
	// produces reads exactly as it did; everything else is shown as the JSON it
	// came in as.
	const shown = (value: Json) => (isString(value) ? value : JSON.stringify(value));

	// Checks one object against a list of allowed properties: `required` must
	// be present and pass `check`, the rest may be absent, and nothing else may
	// appear (the schema's additionalProperties: false).
	/** @param where what the refusal calls it ("queue #3"). */
	const checkMembers = (entry: JsonObject, where: string, props: Record<string, Rule>) => {
		for (const [name, { required, check, expected }] of Object.entries(props)) {
			if (!(name in entry)) {
				if (required) fail(`${where}: “${name}” is missing`);
				continue;
			}

			if (!check(entry[name])) fail(`${where}: “${name}” must be ${expected}`);
		}

		for (const name of Object.keys(entry)) {
			if (!(name in props)) fail(`${where}: “${name}” is not part of this format`);
		}
	};

	// The schema's `knownDetails`: what SoundCloud said about a track.
	const checkKnownDetails = (entry: JsonObject, where: string) =>
		checkMembers(entry, where, {
			id: { check: isIntegerOrNull, expected: 'a whole number or null' },
			permalink: { required: true, check: isPermalink, expected: 'a permalink path' },
			downloadable: { required: true, check: isBoolean, expected: 'true or false' },
			hasDownloadsLeft: { required: true, check: isBoolean, expected: 'true or false' },
			purchaseUrl: { required: true, check: isString, expected: 'a string' },
			purchaseTitle: { required: true, check: isString, expected: 'a string' },
			// Optional, and only optional because of the files that already exist:
			// details cached before the genre was kept have no such key, and a
			// backup taken then still has to read back. '' means the uploader set
			// no genre; absent means nobody has looked since it started being kept.
			genre: { check: isString, expected: 'a string' },
			// The cover SoundCloud named — a track's own artwork, else its
			// uploader's avatar. '' means it has neither. Optional for the same
			// reason as the genre: details cached before the cover was kept have no
			// such key, and a backup taken then still has to read back.
			artworkUrl: { check: isString, expected: 'a string' },
			at: { required: true, check: isTimestamp, expected: 'a time in milliseconds' }
		});

	// The schema's `partialDetails`: what a Likes row shows by itself. The page
	// renders the purchase link there, so that much is known without asking
	// SoundCloud, and the nulls say that nothing else is. Only a queued track
	// carries one — the details cache holds answers, and this is not one, so it
	// is never filed there and never has a time to expire from.
	const checkPartialDetails = (entry: JsonObject, where: string) =>
		checkMembers(entry, where, {
			partial: { required: true, check: (v) => v === true, expected: 'true' },
			id: { check: (v) => v === null, expected: 'null' },
			permalink: { required: true, check: isPermalink, expected: 'a permalink path' },
			downloadable: { required: true, check: (v) => v === null, expected: 'null' },
			hasDownloadsLeft: { required: true, check: (v) => v === null, expected: 'null' },
			purchaseUrl: { required: true, check: isString, expected: 'a string' },
			purchaseTitle: { required: true, check: isString, expected: 'a string' },
			// Null like the two above it, and for the same reason: a Likes row
			// renders no genre, so this entry knows of none. Optional, for the
			// queued tracks in files written before it existed.
			genre: { check: (v) => v === null, expected: 'null' },
			// '' rather than null: the cover was read off the same row this entry
			// was, so there is nothing here the queued track does not already have.
			// Optional, for the queued tracks in files written before it existed.
			artworkUrl: { check: (v) => v === '', expected: 'an empty string' }
		});

	// The schema's `unavailableDetails`: a remembered lookup failure.
	const checkUnavailableDetails = (entry: JsonObject, where: string) =>
		checkMembers(entry, where, {
			unavailable: { required: true, check: (v) => v === true, expected: 'true' },
			at: { required: true, check: isTimestamp, expected: 'a time in milliseconds' }
		});

	// The schema's `details` map.
	const checkDetails = (details: Json) => {
		if (!isObject(details)) fail('“details” must be an object');

		for (const [permalink, entry] of Object.entries(details)) {
			const where = `details ${permalink}`;

			if (!isPermalink(permalink)) fail(`${where}: not a permalink path`);

			if (!isObject(entry)) fail(`${where}: must be an object`);

			if ('unavailable' in entry) {
				checkUnavailableDetails(entry, where);
				continue;
			}

			checkKnownDetails(entry, where);

			// Not something the schema can say: the entry names the track it is
			// filed under. A mismatch would put the wrong details on a track.
			if (entry.permalink !== permalink) {
				fail(`${where}: “permalink” says ${shown(entry.permalink)}, but it is filed under ${permalink}`);
			}
		}
	};

	// The schema's `trackDetails`: what a track carries its own copy of, whether
	// it is sitting in the queue or on the list. Either what SoundCloud said or
	// a Likes row's partial answer — never a remembered failure, which is not an
	// answer about the track at all.
	/** @param permalink the track's own. */
	const checkTrackDetails = (details: JsonObject, where: string, permalink: Json, held: 'queued' | 'listed') => {
		// Saying that is worth a line of its own: checked as known details, such
		// an entry is reported as missing every field it has, which names none of
		// what is actually wrong.
		if ('unavailable' in details) {
			fail(`${where}: a remembered lookup failure is not what a track is ${held} with`);
		}

		if ('partial' in details) checkPartialDetails(details, where);
		else checkKnownDetails(details, where);

		// Not something the schema can say, and the same hazard as a misfiled
		// cache entry: the details a track carries name that track. A mismatch
		// would send the Downloader to another track's gate.
		if (details.permalink !== permalink) {
			fail(`${where}: “permalink” says ${shown(details.permalink)}, but the track ${held} is ${shown(permalink)}`);
		}
	};

	// What the page a track came off said about it, which nothing else can:
	// the genre it rendered beside the row, and whether the row's own like and
	// repost buttons were on. A queued track carries them as well as a listed
	// one — the queue is a source the list adds from, and the page is gone by
	// then — so they belong to the shape both share. All three are optional:
	// null is "the page never said", and so is a file written before they were
	// kept, which still has to read back.
	const PAGE_FACTS = {
		genre: { check: (v) => v === null || isString(v), expected: 'a string or null' },
		liked: { check: (v) => v === null || isBoolean(v), expected: 'true, false or null' },
		reposted: { check: (v) => v === null || isBoolean(v), expected: 'true, false or null' }
	} satisfies Record<string, Rule>;

	// The properties a listed track has over a queued one: the uploader's
	// wording for a purchase link, and the two things the list itself knows —
	// where the track came in from, and when.
	const LISTED_EXTRAS = {
		domPurchaseTitle: { check: isString, expected: 'a string' },
		from: { check: isString, expected: 'a string' },
		at: { check: isTimestamp, expected: 'a time in milliseconds' }
	} satisfies Record<string, Rule>;

	// One array of track entries — the queue, or the Manager's list — against
	// the shape they share. The one thing an entry cannot go without is the
	// permalink: it is the identity for a track everywhere in this extension,
	// what gets marked when its turn comes, and what tells two entries apart.
	const checkTracks = (
		list: Json,
		name: 'queue' | 'listed',
		held: 'queued' | 'listed',
		extras: Record<string, Rule> = {}
	) => {
		if (!Array.isArray(list)) fail(`“${name}” must be an array`);
		list.forEach((entry, index) => {
			const where = `${name} #${index + 1}`;

			if (!isObject(entry)) fail(`${where}: must be an object`);
			checkMembers(entry, where, {
				permalink: { required: true, check: isPermalink, expected: 'a permalink path' },
				title: { check: isString, expected: 'a string' },
				artist: { check: isString, expected: 'a string' },
				artworkUrl: { check: isString, expected: 'a string' },
				details: {
					check: (v) => v === null || isObject(v),
					expected: 'download details or null'
				},
				...PAGE_FACTS,
				...extras
			});

			// Null or an object by now: checkMembers has just said so.
			if (isObject(entry.details)) {
				checkTrackDetails(entry.details, `${where} details`, entry.permalink, held);
			}
		});
	};

	// The schema's `queue`: the session queue, in the order its tracks were
	// added. Its details are the ones the track was queued with, which may be a
	// Likes row's partial ones.
	const checkQueue = (queue: Json) => checkTracks(queue, 'queue', 'queued');

	// The schema's `listed`: the Manager's own list, in the order tracks were
	// put on it. A queue entry plus what only the list knows — see LISTED_EXTRAS.
	const checkListed = (list: Json) => checkTracks(list, 'listed', 'listed', LISTED_EXTRAS);

	// The schema's `profileLink`: one link an account lists on its profile.
	const checkProfileLink = (link: Json, where: string) => {
		if (!isObject(link)) fail(`${where}: must be an object`);
		checkMembers(link, where, {
			network: { check: isString, expected: 'a string' },
			title: { check: isString, expected: 'a string' },
			url: { required: true, check: isString, expected: 'a string' }
		});
	};

	// The schema's `artistProfile`: what SoundCloud said about an account.
	const checkArtistProfile = (profile: JsonObject, where: string) => {
		checkMembers(profile, where, {
			id: { check: isIntegerOrNull, expected: 'a whole number or null' },
			permalink: { check: isArtistPath, expected: 'an artist’s path' },
			username: { check: isString, expected: 'a string' },
			fullName: { check: isString, expected: 'a string' },
			city: { check: isString, expected: 'a string' },
			country: { check: isString, expected: 'a string' },
			followers: { check: isIntegerOrNull, expected: 'a whole number or null' },
			trackCount: { check: isIntegerOrNull, expected: 'a whole number or null' },
			verified: { check: isBoolean, expected: 'true or false' },
			avatarUrl: { check: isString, expected: 'a string' },
			bio: { check: isString, expected: 'a string' },
			links: { check: Array.isArray, expected: 'an array' },
			at: { required: true, check: isTimestamp, expected: 'a time in milliseconds' }
		});

		// An array or absent by now: checkMembers has just said so.
		if (Array.isArray(profile.links)) {
			profile.links.forEach((link, index) => checkProfileLink(link, `${where} link #${index + 1}`));
		}
	};

	// The schema's `profileEntry`: a profile, or a remembered failure to find
	// one. Only ever given an object — the entry's own shape check refuses
	// anything else before this is reached.
	const checkProfile = (profile: JsonObject, where: string) => {
		if ('unavailable' in profile) checkUnavailableDetails(profile, where);
		else checkArtistProfile(profile, where);
	};

	// The schema's `artists`: the artists tool's list, in the order artists
	// were put on it. The one thing an entry cannot go without is the path,
	// which is the identity for an artist everywhere in that tool.
	const checkArtists = (list: Json) => {
		if (!Array.isArray(list)) fail('“artists” must be an array');
		list.forEach((entry, index) => {
			const where = `artists #${index + 1}`;

			if (!isObject(entry)) fail(`${where}: must be an object`);
			checkMembers(entry, where, {
				permalink: { required: true, check: isArtistPath, expected: 'an artist’s path' },
				name: { check: isString, expected: 'a string' },
				tracks: { check: Array.isArray, expected: 'an array' },
				profile: { check: (v) => v === null || isObject(v), expected: 'a profile or null' },
				from: { check: isString, expected: 'a string' },
				at: { check: isTimestamp, expected: 'a time in milliseconds' }
			});

			// As with a profile's links: an array or absent, checkMembers has just said.
			if (Array.isArray(entry.tracks)) {
				entry.tracks.forEach((track, n) => {
					const at = `${where} track #${n + 1}`;

					if (!isObject(track)) fail(`${at}: must be an object`);
					checkMembers(track, at, {
						permalink: { required: true, check: isPermalink, expected: 'a permalink path' },
						title: { check: isString, expected: 'a string' }
					});
				});
			}

			// Null or an object, likewise.
			if (isObject(entry.profile)) checkProfile(entry.profile, `${where} profile`);
		});
	};

	// The schema's `downloaded` map.
	const checkDownloaded = (downloaded: Json) => {
		if (!isObject(downloaded)) fail('“downloaded” must be an object');

		for (const [permalink, at] of Object.entries(downloaded)) {
			if (!isPermalink(permalink)) fail(`downloaded ${permalink}: not a permalink path`);

			if (!isDateTime(at)) fail(`downloaded ${permalink}: “${shown(at)}” is not a time`);
		}
	};

	// The schema's `prefs` object.
	const checkPrefs = (prefs: Json) => {
		if (!isObject(prefs)) fail('“prefs” must be an object');

		for (const [name, value] of Object.entries(prefs)) {
			const pref = PREFS.find((p) => p.name === name);

			if (!pref) fail(`prefs: “${name}” is not an option of this tool`);

			if (pref.type === 'boolean' && !isBoolean(value)) {
				fail(`prefs: “${name}” must be true or false`);
			}

			if (pref.type === 'enum' && !pref.values.some((allowed) => allowed === value)) {
				fail(`prefs: “${name}” must be ${pref.values.map((v) => `“${v}”`).join(' or ')}`);
			}
		}
	};

	// The whole document, against the whole schema. Typed on the binding, as
	// `fail` is, so a call narrows what follows it.
	const checkState: (doc: Json) => asserts doc is JsonObject & StateFile = (doc) => {
		if (!isObject(doc)) fail('the file must be a JSON object.');

		if (!READS.some((format) => format === doc.format)) {
			fail(
				doc.format === undefined
					? 'the file doesn’t say what it is — it wasn’t exported by this tool.'
					: `that’s a “${shown(doc.format)}”, not a Manager state file.`
			);
		}

		if (doc.version !== VERSION) {
			fail(`that file is version ${JSON.stringify(doc.version)}; this reads version ${VERSION}.`);
		}

		for (const name of Object.keys(doc)) {
			if (
				!['format', 'version', 'exportedAt', 'downloaded', 'details', 'listed', 'artists', 'queue', 'prefs'].includes(
					name
				)
			) {
				fail(`“${name}” is not part of this format`);
			}
		}

		if ('exportedAt' in doc && !isDateTime(doc.exportedAt)) {
			fail('“exportedAt” is not a time');
		}

		if ('downloaded' in doc) checkDownloaded(doc.downloaded);

		if ('details' in doc) checkDetails(doc.details);

		// Absent in every file written before the Manager kept a list of its own,
		// and those still read: an import then restores the marks and the lookups
		// as it always did, and leaves the list as it is.
		if ('listed' in doc) checkListed(doc.listed);

		// Newer still than the list, and absent from every file before it.
		if ('artists' in doc) checkArtists(doc.artists);

		if ('queue' in doc) checkQueue(doc.queue);

		if ('prefs' in doc) checkPrefs(doc.prefs);
	};

	// --- parsing ---------------------------------------------------------------

	// Reads exported JSON, refusing anything the schema would refuse. Returns
	// the document with its six collections filled in, so callers never
	// have to check for them. Throws StateError with a message naming what is
	// wrong; nothing is written before this has passed.
	const parse = (text: string): ParsedState => {
		let doc: Json;

		try {
			doc = JSON.parse(text);
		} catch (_e) {
			fail('that’s not JSON. export a file from this panel, then import that.');
		}

		checkState(doc);

		return {
			...doc,
			downloaded: doc.downloaded || {},
			details: doc.details || {},
			listed: doc.listed || [],
			artists: doc.artists || [],
			queue: doc.queue || [],
			prefs: doc.prefs || {}
		};
	};

	// --- exporting -------------------------------------------------------------

	const readPrefs = (): StatePrefs => {
		const prefs: Record<string, boolean | string> = {};

		try {
			for (const pref of PREFS) {
				const stored = localStorage.getItem(pref.key);

				if (stored === null) continue;

				if (pref.type === 'boolean') prefs[pref.name] = stored === '1';
				else if (pref.values.includes(stored)) prefs[pref.name] = stored;
			}
		} catch (_e) {
			// Storage unavailable — the file simply carries no options.
		}

		return prefs;
	};

	// Everything worth keeping, as the object that gets written to the file.
	//
	// The queue and the two lists are handed in rather than read out of
	// storage, because the stores that own them do not always get to write them
	// there: when storage cannot be reached each is held in memory and shown on
	// screen all the same (queue.svelte.js, listed.svelte.js,
	// artists.svelte.js), and a file written from storage would then carry
	// nothing while the panel showed a full list. All three have to be plain
	// data — the stores' `entries`, not the panel's `$state` — since they go
	// straight into the file. A caller with none to give still gets a valid
	// document, which is the same bargain the options make.
	const snapshot = async (
		queue: QueueEntry[] = [],
		listed: ListedEntry[] = [],
		artists: ArtistEntry[] = []
	): Promise<Required<StateFile>> => {
		const stored: Stored = await chrome.storage.local.get([TbxDownloaded.KEY, DETAILS_KEY]);

		return {
			format: FORMAT,
			version: VERSION,
			exportedAt: new Date().toISOString(),
			downloaded: stored[TbxDownloaded.KEY] || {},
			details: stored[DETAILS_KEY] || {},
			listed,
			artists,
			queue,
			prefs: readPrefs()
		};
	};

	// The file's contents, and the name to offer it under.
	const serialize = (state: StateFile) => JSON.stringify(state, null, 2);

	const filename = (now = new Date()) => `manager-${now.toISOString().slice(0, 10)}.json`;

	// What a parsed document holds, for the box that says what is about to be
	// imported before a byte of it is written. Counted from the filled-in
	// document `parse` answers with, so every member is there to count.
	const summary = (doc: ParsedState) => ({
		marks: Object.keys(doc.downloaded).length,
		details: Object.keys(doc.details).length,
		listed: doc.listed.length,
		artists: doc.artists.length,
		queue: doc.queue.length,
		prefs: Object.keys(doc.prefs).length,
		exportedAt: doc.exportedAt ?? ''
	});

	// --- importing -------------------------------------------------------------

	// Writes a parsed document into storage, in one of two ways.
	//
	// Adding — which is what happens unless somebody asks for the other — never
	// removes anything: marks are added (the store keeps the earlier time for
	// one already there), details are taken only where they are newer than what
	// is known (the same rule the content script applies to another tab's
	// lookups), and an option the file carries is set only where nothing is set
	// already. That last one is the point of the rule rather than an exception
	// to it: an option somebody has chosen here is a thing they have, and an
	// import that is adding to what they have does not overwrite it.
	//
	// Replacing is the opt-in, and it is the exact opposite in every part: the
	// marks become the file's marks, the details cache becomes the file's cache,
	// and every option goes to what the file says — or, where the file says
	// nothing, back to its default. What was here and is not in the file is
	// gone. Nobody gets there without turning a switch on first.
	//
	// Either way, neither the file's queue nor its lists are written here, for
	// the same reason the marks are not: the store that owns a thing does the
	// writing. The sheet hands `doc.queue`, `doc.listed` and `doc.artists` to
	// those stores' own `add` (or `replace`), and folds the counts it gets back
	// into the same report.
	//
	// Returns what changed, for the sheet to report.
	const apply = async (doc: ParsedState, { replace = false }: { replace?: boolean } = {}) => {
		const marks = replace ? await TbxDownloaded.replaceAll(doc.downloaded) : await TbxDownloaded.merge(doc.downloaded);

		let details = 0;

		if (replace) {
			details = Object.keys(doc.details).length;
			await chrome.storage.local.set({ [DETAILS_KEY]: doc.details });
		} else {
			const entries = Object.entries(doc.details);

			if (entries.length > 0) {
				const held: Stored = await chrome.storage.local.get(DETAILS_KEY);
				const stored = held[DETAILS_KEY] || {};

				for (const [permalink, entry] of entries) {
					const known = stored[permalink];

					if (known && known.at >= entry.at) continue;
					stored[permalink] = entry;
					details++;
				}

				if (details > 0) await chrome.storage.local.set({ [DETAILS_KEY]: stored });
			}
		}

		let prefs = 0;

		try {
			for (const pref of PREFS) {
				const given = pref.name in doc.prefs;

				if (!given) {
					// Replacing means the whole set, so an option the file is silent
					// about goes back to whatever it is when nobody has set it.
					if (replace && localStorage.getItem(pref.key) !== null) {
						localStorage.removeItem(pref.key);
						prefs++;
					}

					continue;
				}

				// Adding leaves a choice already made alone; see above.
				if (!replace && localStorage.getItem(pref.key) !== null) continue;
				const value = doc.prefs[pref.name];
				localStorage.setItem(pref.key, pref.type === 'boolean' ? (value ? '1' : '0') : String(value));
				prefs++;
			}
		} catch (_e) {
			// Storage unavailable — the options stay as they are.
		}

		return { marks, details, prefs };
	};

	return {
		FORMAT,
		READS,
		VERSION,
		DETAILS_KEY,
		LISTED_KEY,
		ARTISTS_KEY,
		PREFS,
		StateError,
		parse,
		summary,
		snapshot,
		serialize,
		filename,
		apply
	};
})();
