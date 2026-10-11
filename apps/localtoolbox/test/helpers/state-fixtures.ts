// The exported state file and the entries it carries, with defaults that
// make a valid document — for the tests of state-transfer.ts, of the schema
// (docs/manager-state.schema.json), and of the settings sheet that imports and
// exports one.
//
// A module of its own rather than exports from test/state-transfer.test.ts:
// importing a test file registers every test in it again, in the importing
// file, so the schema's tests used to run the parser's a second time.
//
// A builder's type is the shape it makes when left alone. What it is handed on top may be any
// field with any value, since many of these documents are made for the schema to refuse.
import type {
	ArtistEntry,
	KnownDetails,
	KnownProfile,
	ListedEntry,
	PartialDetails,
	QueueEntry,
	StateFile
} from '$lib/types';

/** What one of these builders makes. */
type Built = KnownDetails | PartialDetails | QueueEntry | ListedEntry | KnownProfile | ArtistEntry | StateFile;

/** A field's value as a builder may be handed one: what another builder made, or any JSON. */
export type FieldValue =
	| string
	| number
	| boolean
	| null
	| undefined
	| Built
	| FieldValue[]
	| { [key: string]: FieldValue };

/** The fields laid over a builder's defaults. */
export type Overrides = Record<string, FieldValue>;

export const ISO = '2026-09-08T12:00:00.000Z';

export const AT = 1757332800000;

// A known-details entry, and the file that carries one.
export const knownDetails = (extra: Overrides = {}): KnownDetails => ({
	id: 42,
	permalink: '/bicep/glue',
	downloadable: true,
	hasDownloadsLeft: true,
	purchaseUrl: 'https://hypeddit.com/x',
	purchaseTitle: 'Free Download',
	genre: 'Techno',
	at: AT,
	...extra
});

// The details a Likes row shows by itself: the purchase link, and nulls for
// everything only the API could answer. Only a queued track carries these.
export const partialDetails = (extra: Overrides = {}): PartialDetails => ({
	id: null,
	permalink: '/bicep/glue',
	downloadable: null,
	hasDownloadsLeft: null,
	purchaseUrl: 'https://hypeddit.com/x',
	purchaseTitle: 'Free Download',
	genre: null,
	partial: true,
	...extra
});

// A queued track, as the queue keeps one — the three the page it came off is
// the only source of included, since the list adds from the queue and the
// page is gone by then.
export const queueEntry = (extra: Overrides = {}): Required<QueueEntry> => ({
	permalink: '/bicep/glue',
	title: 'Glue',
	artist: 'Bicep',
	artworkUrl: 'https://i1.sndcdn.com/artworks-abc-t120x120.jpg',
	details: knownDetails(),
	genre: 'Techno',
	liked: true,
	reposted: null,
	...extra
});

// A track on the Manager's list: a queued track plus what only the list
// knows — the uploader's own wording for a purchase link, and where and when
// it came in.
export const listedEntry = (extra: Overrides = {}): ListedEntry => ({
	...queueEntry(),
	domPurchaseTitle: 'Free Download',
	from: 'My Playlist',
	at: AT,
	...extra
});

// What SoundCloud said about an account, as the artists tool keeps it.
export const artistProfile = (extra: Overrides = {}): KnownProfile => ({
	id: 7,
	permalink: '/bicep',
	username: 'Bicep',
	fullName: 'Andy & Matt',
	city: 'Belfast',
	country: 'GB',
	followers: 1234,
	trackCount: 42,
	verified: true,
	avatarUrl: 'https://i1.sndcdn.com/avatars-abc-large.jpg',
	bio: 'bookings: agent@example.com',
	links: [{ network: 'instagram', title: 'Instagram', url: 'https://www.instagram.com/bicepmusic/' }],
	at: AT,
	...extra
});

// An artist on the artists tool's list: the uploader behind a track, the
// tracks that brought it in, what SoundCloud said, and where and when it came
// in.
export const artistEntry = (extra: Overrides = {}): ArtistEntry => ({
	permalink: '/bicep',
	name: 'Bicep',
	tracks: [{ permalink: '/bicep/glue', title: 'Glue' }],
	profile: artistProfile(),
	from: 'My Playlist',
	at: AT,
	...extra
});

export const file = (extra: Overrides = {}): StateFile => ({
	format: 'toolbox.manager.state',
	version: 1,
	exportedAt: ISO,
	downloaded: { '/bicep/glue': ISO },
	details: { '/bicep/glue': knownDetails() },
	listed: [listedEntry()],
	artists: [artistEntry()],
	queue: [queueEntry()],
	prefs: { background: true, sort: 'page', hideDownloaded: false },
	...extra
});
