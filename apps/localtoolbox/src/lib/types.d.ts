// The shapes that cross a module boundary: what the popup and the content script say to each
// other, what a track is at each hop between the page and the Manager's list, what is kept in
// chrome.storage, the tool registry and the exported state file. Everything else is typed where
// it lives.
//
// Nothing imports this at runtime. src imports its types from `$lib/types`; the classic scripts at
// the root, which cannot import, reach them as the global namespace `Tbx` (script-types.d.ts).
//
// Each shape is what the code writes. Where the state file lets a field be absent
// (docs/manager-state.schema.json is the spec) and a store keeps what the file gave it as it is,
// the field is optional here too; where a store rebuilds what it is given (the lists' `entryOf`),
// the stored shape is whole and the file's is a `Saved` one of its own.
//
// A field one kind of a union never sets is declared `?: undefined`, so it can be asked of either
// kind. Not `?: never`: without strictNullChecks that narrows the falsy branch to the other kind.

import type { Component } from 'svelte';
import type { LucideIcon } from '@lucide/svelte';

/** `T` with only the `K` fields sure to be there, which is how the state file allows most. */
export type Loose<T, K extends keyof T> = Pick<T, K> & Partial<Omit<T, K>>;

// ---- The page ---------------------------------------------------------------

/** What kind of page the content script found rendered (content.ts detectPage). */
export type PageType = 'playlist' | 'likes' | 'track' | 'other';

/** What only the page a track came off can say. Null is "the page never said", not "no". */
export interface PageFacts {
	/** The genre tag the row drew beside its title, or null where it drew none. */
	genre: string | null;
	/** Whether the row's own like button was on, or null where the row drew none. */
	liked: boolean | null;
	/** The same, for the repost button. */
	reposted: boolean | null;
}

/** A row's words and picture as its queue button reads them; the player bar draws no facts. */
export interface RowFields extends Partial<PageFacts> {
	title: string;
	artist: string;
	/** The thumbnail's URL, or '' where it never lazy-loaded. */
	artworkUrl: string;
}

/** A track as the page renders it (content.ts extract): what EXTRACT_TRACKS lists. */
export interface PageTrack extends PageFacts {
	/** The track's path, '/<user>/<track>'; '' for a title that is no link, or an unnamed track. */
	permalink: string;
	title: string;
	artist: string;
	/** The thumbnail's URL, or '' where it never lazy-loaded. */
	artworkUrl: string;
	/** Its place among every liked item, playlists too. Likes rows and track pages only. */
	itemIndex?: number;
	/** The purchase link the page drew, out of gate.sc. Likes rows and track pages only. */
	domPurchaseUrl?: string;
	/** The uploader's label for that link ("Free Download"), or ''. */
	domPurchaseTitle?: string;
}

/** A page's track with what is known of its downloads; a missing cover is taken from them. */
export interface DetailedTrack extends PageTrack {
	/** Null when the page has not loaded the track yet. */
	details?: TrackDetails | null;
}

// ---- Download details -------------------------------------------------------

/** What the page loaded about a track, as content.ts files it (detailsOf). */
export interface KnownDetails {
	/** SoundCloud's numeric id. The page always names one; a restored file need not. */
	id?: number | null;
	/** The track's path: the one the entry is filed under, or the track carrying it. */
	permalink: string;
	/** Whether SoundCloud itself offers "Download file". */
	downloadable: boolean;
	/** Whether that download's limit is still unspent. */
	hasDownloadsLeft: boolean;
	/** The purchase link the uploader set, or ''. */
	purchaseUrl: string;
	/** The uploader's label for it, or ''. */
	purchaseTitle: string;
	/** '' is "none set". Absent only in details kept before the genre was. */
	genre?: string;
	/** The track's artwork, else the uploader's avatar, else ''. Absent in details that old. */
	artworkUrl?: string;
	/** When it was learned, in milliseconds. */
	at: number;
	/** Never set; declared so `details.partial` can be asked of either kind. */
	partial?: undefined;
	/** Never set; declared so `entry.unavailable` can be asked of a cache entry. */
	unavailable?: undefined;
}

/** What a Likes row or track page shows by itself: the purchase link, and nothing asked. */
export interface PartialDetails {
	partial: true;
	/** The id comes with the track object, which the page has not loaded. Absent in older files. */
	id?: null;
	permalink: string;
	downloadable: null;
	hasDownloadsLeft: null;
	purchaseUrl: string;
	purchaseTitle: string;
	/** The row's own tag is on the track, not here. Absent in older files. */
	genre?: null;
	/** The cover came off the same row as this did. Absent in older files. */
	artworkUrl?: '';
}

/** A lookup that came back with nothing, remembered so it is not asked again at once. */
export interface RememberedFailure {
	unavailable: true;
	/** When it failed, in milliseconds. */
	at: number;
	/** Never set; declared so `entry.id` can be asked of any cache entry (content.ts). */
	id?: undefined;
}

/** The details a track carries: an answer or a partial one, never a failure. */
export type TrackDetails = KnownDetails | PartialDetails;

/** One entry of an older version's details cache: an answer, or a remembered failure. */
export type DetailsCacheEntry = KnownDetails | RememberedFailure;

/** An older version's details cache, permalink -> entry, as its state files carry it. */
export type DetailsCache = Record<string, DetailsCacheEntry>;

// ---- A track, hop by hop ----------------------------------------------------
//
// The page (PageTrack, or RowFields off one row) -> the session queue (QueueEntry) -> the
// Manager's list (ListedEntry), which adds from the page directly too -> the state file. A field
// one hop does not name is gone for good by the next, since the page it came off is gone by then.

/** A track in the session queue, as the popup or a page's queue or download button wrote it. */
export interface QueueEntry {
	/** The track's path, '/<user>/<track>': the one thing an entry cannot go without. */
	permalink: string;
	/** '' where the row named none; absent where a page's button found no row, or a file had none. */
	title?: string;
	/** As the title: '' where the row named none, absent where there was no row or no field. */
	artist?: string;
	artworkUrl?: string;
	/** What was known when it was queued, which is how the Downloader finds its gate. */
	details?: TrackDetails | null;
	/** Null where the page never said; absent in a queue written before these were kept. */
	genre?: string | null;
	liked?: boolean | null;
	reposted?: boolean | null;
}

/** Any track a panel holds or a store takes in, from whichever hop: only the permalink is sure. */
export interface Track extends QueueEntry {
	/** The uploader's label for the purchase link, where the page or the list carries one. */
	domPurchaseTitle?: string;
}

/** A track on the Manager's list, whole, as listed.svelte.ts entryOf writes it. */
export interface ListedEntry extends PageFacts {
	permalink: string;
	/** '' where the page named none. */
	title: string;
	/** '' where the page named none. */
	artist: string;
	/** '' where the page showed none; the details' cover stands in (artwork.ts artworkOf). */
	artworkUrl: string;
	/** The better of every answer seen (betterDetails), or null when nobody has asked. */
	details: TrackDetails | null;
	/** The uploader's label for the purchase link, for when the details carry none. */
	domPurchaseTitle: string;
	/** A page's title or kind ("a likes page"), or "the session queue"; '' when nothing named it. */
	from: string;
	/** When it was first added, in milliseconds. A second sighting does not move it. */
	at: number;
}

// ---- Artists ----------------------------------------------------------------

/** One link an account lists on its profile (content.ts profileLinksOf). */
export interface ProfileLink {
	/** What SoundCloud files it under ('instagram', 'personal', …), lower-cased, or ''. */
	network: string;
	/** The uploader's own title for it, or ''. */
	title: string;
	url: string;
}

/** An account as the page loaded it, as GET_ARTIST_PROFILES answers (content.ts profileOf). */
export interface ArtistProfile {
	id: number | null;
	/** The artist's path, '/<user>', as it was asked for. */
	permalink: string;
	/** SoundCloud's display name for the account, which a Likes row need not show. */
	username: string;
	fullName: string;
	city: string;
	/** A country code ('GB'), or ''. */
	country: string;
	/** Null where SoundCloud did not say. */
	followers: number | null;
	/** Null where SoundCloud did not say. */
	trackCount: number | null;
	verified: boolean;
	avatarUrl: string;
	/** Cut at a thousand characters. The panel reads links out of it too. */
	bio: string;
	/** At most twenty, http(s) only, in SoundCloud's order. */
	links: ProfileLink[];
	/** Set where the bio or the links have not been loaded: they come with the profile page. */
	partial?: true;
	/** Never set; declared so `profile.unavailable` can be asked of any answer. */
	unavailable?: undefined;
}

/** A profile as the artists list keeps it: stamped on arrival, or as a restored file had it. */
export interface KnownProfile extends Partial<Omit<ArtistProfile, 'links'>> {
	links?: Loose<ProfileLink, 'url'>[];
	/** When it arrived, in milliseconds. */
	at: number;
}

/** What the artists list knows of an account: a profile, or that there was none to find. */
export type ProfileEntry = KnownProfile | ProfileFailure;

/**
 * No profile behind the path, remembered. A profile's fields are declared never set, so the
 * artists panel can ask any of them of either kind (`entry.profile?.username`).
 */
export type ProfileFailure = RememberedFailure &
	Partial<Record<Exclude<keyof KnownProfile, keyof RememberedFailure>, undefined>>;

/** One of the tracks an artist is on the list for. */
export interface ArtistTrack {
	permalink: string;
	/** As the page showed it, or ''. */
	title: string;
}

/** The uploader behind some tracks, one per path (profiles.ts artistsOf). */
export interface Artist {
	/** '/<user>', the first segment of every one of their tracks' permalinks. */
	permalink: string;
	/** What the page called the artist, or '' where no row named one. */
	name: string;
	/** The tracks that brought it in, in the order seen; the store keeps fifty. */
	tracks: ArtistTrack[];
}

/** An artist on the artists list, as artists.svelte.ts entryOf writes it. */
export interface ArtistEntry extends Artist {
	/** Null until a SoundCloud tab has loaded the account. Kept for the session only (artists.svelte.ts). */
	profile: ProfileEntry | null;
	/** A page's name, "the Manager’s list" or "the session queue"; '' when nothing named it. */
	from: string;
	/** When it was first added, in milliseconds. */
	at: number;
}

// ---- What a panel asks the page ---------------------------------------------
//
// Asked with chrome.tabs.sendMessage (chrome.ts askPage), and answered by the tab's top frame
// only. A tab open across an update still runs the content script it loaded with, which can
// answer in any shape at all: these are what this version says, and readers still check.

/** A question for the content script. */
export type PageRequest =
	| { type: 'EXTRACT_TRACKS' }
	| { type: 'GET_DOWNLOAD_DETAILS' }
	| { type: 'GET_ARTIST_PROFILES'; permalinks: string[] };

/** The answer to EXTRACT_TRACKS: the page read off its DOM, nothing asked of SoundCloud. */
export interface TracksAnswer {
	ok: true;
	url: string;
	pageType: PageType;
	/** The playlist, Likes page or track as a path; '' for another page or an unnamed track. */
	pageKey: string;
	/** 'url' when nothing recognisable was rendered and the address had to do. */
	pageDetectedBy: 'dom' | 'url';
	/** The hero's title, a Likes page's heading ('your likes' for your own), or ''. */
	title: string;
	/** The uploader or curator the hero names, or ''. */
	curator: string;
	tracks: PageTrack[];
	/** Liked playlists on a Likes page, which are counted rather than listed. */
	skippedPlaylists: number;
}

/** The answer to GET_DOWNLOAD_DETAILS: the page again, each track with its details. */
export interface DetailsAnswer extends TracksAnswer {
	tracks: DetailedTrack[];
}

/** The answer to GET_ARTIST_PROFILES: the accounts the page has loaded, of those asked about. */
export interface ProfilesAnswer {
	ok: true;
	profiles: ArtistProfile[];
}

/** Each question's answer, by its type: what askPage's `response` holds when one came. */
export interface PageAnswers {
	EXTRACT_TRACKS: TracksAnswer;
	GET_DOWNLOAD_DETAILS: DetailsAnswer;
	GET_ARTIST_PROFILES: ProfilesAnswer;
}

// ---- What a replaced download button asks the extension ---------------------
//
// Sent with chrome.runtime.sendMessage. The page asks the background worker (background.ts),
// which has the gate walked in the offscreen page (offscreen.ts) and asks nothing of a panel.

/** A gate link the page's Download button wants saved. */
export interface SaveGateRequest {
	type: 'SAVE_GATE';
	url: string;
}

/** What became of it: saved (and whether that marks the track), or opened in a tab instead. */
export interface SaveGateAnswer {
	saved: boolean;
	mark: boolean;
}

/** The worker's errand for the offscreen page: one gate walked to its file. */
export interface WalkGateRequest {
	type: 'WALK_GATE';
	url: string;
}

/** The end of a walk: the file and where it goes, or the page to send the person to instead. */
export type WalkGateAnswer =
	| { ok: true; url: string; filename: string; subfolder: string; mark: boolean }
	| { ok: false; openUrl: string };

/** The offscreen page's errand for the worker: a sign-in walked in a tab (chrome.ts roundTrip). */
export interface RoundTripRequest {
	type: 'ROUND_TRIP';
	url: string;
	landing: string;
}

/** A read of the page (readPage, lookupProfiles): its answer, or the sentence for why not. */
export type PageRead<T> = { data: T; status?: undefined } | { status: string; data?: undefined };

// ---- The tools --------------------------------------------------------------

/** A tool as the registry lists it ($lib/tools.ts TOOLS). */
export interface Tool {
	/** Its route, and the folder its panel lives in: src/tools/<id>/. */
	id: string;
	name: string;
	/** The name where there is room for a word or two (the queue sheet's send-to buttons). */
	short: string;
	description: string;
	icon: LucideIcon;
	/** Its icon tile's background: a `bg-*` class for one of the DJ-data colours. */
	tone: string;
	/** Fetches its panel's chunk. */
	load: () => Promise<{ default: Component<PanelProps> }>;
	/** Starts the slow thing its panel does first, before the chunk has even landed. */
	warm?: () => void;
}

/** What App.svelte hands every tool's panel. */
export interface PanelProps {
	tool: Tool;
	/** Back to the menu. */
	onback: () => void;
}

// ---- What is kept -----------------------------------------------------------

/** The marks: permalink -> the ISO time it was marked. */
export type DownloadedMarks = Record<string, string>;

/** The settings the content script obeys, written by the popup's settings sheet. */
export interface PageSettings {
	/** Put the Manager's own download button where SoundCloud's are. */
	replaceDownloadButtons: boolean;
	/** Press /you/likes into its List view. */
	likesListView: boolean;
}

/** chrome.storage.local, key by key, as this version writes it. Readers still check. */
export interface LocalArea {
	downloaded: DownloadedMarks;
	listed: ListedEntry[];
	artists: ArtistEntry[];
	settings: PageSettings;
}

/** The chrome.storage.session key the page shares; the popup's own are typed where they live. */
export interface SessionArea {
	queue: QueueEntry[];
}

// ---- The state file ---------------------------------------------------------

/** A listed track as the file may hold one; listed.svelte.ts entryOf fills in the rest. */
export type SavedListedEntry = Loose<ListedEntry, 'permalink'>;

/** An artist as the file may hold one; artists.svelte.ts entryOf fills in the rest. */
export type SavedArtistEntry = Loose<Omit<ArtistEntry, 'tracks'>, 'permalink'> & {
	tracks?: Loose<ArtistTrack, 'permalink'>[];
};

/** A search site as src/lib/search-sites.schema.json describes one: a shipped one, or one added in settings. */
export interface SearchSiteEntry {
	id: string;
	name: string;
	badge: string;
	/** https, with {artist}, {title} or {query} in it. */
	url: string;
	/** Hex colours; a badge without them is grey with white on it. */
	background?: string;
	color?: string;
	/** false ships the badge switched off. */
	on?: boolean;
}

/** The panel options a file carries, from the popup's localStorage `pref:` keys. */
export interface StatePrefs {
	background?: boolean;
	sort?: 'site' | 'page';
	hideDownloaded?: boolean;
}

/** An exported state file, as far as the schema allows one to go without. */
export interface StateFile {
	/** The second is the tool's name from before it was the Manager, read as the first. */
	format: 'toolbox.manager.state' | 'toolbox.soundcloud-downloads.state';
	version: 1;
	exportedAt?: string;
	downloaded?: DownloadedMarks;
	/** An older version's download details. Read, checked, and left out (state-transfer.ts apply). */
	details?: DetailsCache;
	listed?: SavedListedEntry[];
	artists?: SavedArtistEntry[];
	queue?: QueueEntry[];
	prefs?: StatePrefs;
	/** The search sites somebody added in settings; the shipped ones are not in it. */
	searchSites?: SearchSiteEntry[];
}

/** A state file as TbxState.parse answers it: every member but `exportedAt` filled in. */
export interface ParsedState extends StateFile {
	downloaded: DownloadedMarks;
	listed: SavedListedEntry[];
	artists: SavedArtistEntry[];
	queue: QueueEntry[];
	prefs: StatePrefs;
	searchSites: SearchSiteEntry[];
}
