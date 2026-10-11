// Content script for the SoundCloud tools ("Playlist to Text", "Cover Art",
// "Manager").
//
// Runs in Chrome's isolated world: it can see the page's DOM but shares no
// JavaScript state with the page and never executes code in the page's own
// context. For the text and cover-art tools it is a passive observer — it
// reads elements on demand (when the popup asks) and mutates nothing.
//
// The Manager tool adds three things, each in its own section below:
//   - "Download details": on demand, finds out whether each listed track can
//     be downloaded on SoundCloud and where its purchase link points — from
//     data already in the page wherever possible, from SoundCloud's public
//     API (anonymously, in bulk, cached for a month) only for the rest. One
//     lookup runs at a time; a request that arrives while one is under way
//     joins it instead of competing with it;
//   - "Markers": paints an overlay on the cover art of tracks the user has
//     downloaded, and a "nodl" pill on tracks known to have no download link
//     — purely visual DOM additions;
//   - "The queue button": a button on cover art that puts that track in the
//     session queue, or takes it back out — and, for a track already in the
//     queue, stays up to say so. The one thing here that writes anything, and
//     it writes to the extension's own session storage, never to the page;
//   - "Noticing SoundCloud's own download buttons": sees clicks on the page's
//     "Download file" and purchase-link buttons so those downloads get marked
//     too, without interfering with the click. Nothing is sent to SoundCloud.
//
// Two settings, written by the popup's settings sheet and read here, change
// what it does to a page: SoundCloud's own download buttons are replaced with
// this extension's ("Replacing SoundCloud's own download buttons"), and the
// signed-in user's Likes page is pressed into its List view ("The Likes page's
// view"). The second is the one thing this script asks a page to redraw, and
// it asks by pressing SoundCloud's own toggle; it is off unless turned on.
//
// Supported pages: playlist pages (/<user>/sets/<name>, /discover/sets/...)
// and Likes pages (/you/likes, /<user>/likes). Which kind of page is showing
// is read from the rendered content, not from the URL: SoundCloud is a
// single-page app whose address bar and content can disagree (see "Which page
// is this?").
//
// Assumption: the playlist or Likes page is already fully loaded. No scrolling
// or lazy-load triggering is performed; whatever track rows exist in the DOM
// are extracted.
//
// Depends on the TbxDownloaded global from downloaded-store.ts, loaded first.

(() => {
	// The shared shapes this script produces or reads (src/lib/types.d.ts), then its own.
	type PageType = Tbx.PageType;

	type PageFacts = Tbx.PageFacts;

	type RowFields = Tbx.RowFields;

	type PageTrack = Tbx.PageTrack;

	type KnownDetails = Tbx.KnownDetails;

	type TrackDetails = Tbx.TrackDetails;

	type DetailsCacheEntry = Tbx.DetailsCacheEntry;

	type QueueEntry = Tbx.QueueEntry;

	type ProfileLink = Tbx.ProfileLink;

	type ArtistProfile = Tbx.ArtistProfile;

	type PageRequest = Tbx.PageRequest;

	type TracksAnswer = Tbx.TracksAnswer;

	type DetailsAnswer = Tbx.DetailsAnswer;

	type ProfilesAnswer = Tbx.ProfilesAnswer;

	type DetailsProgress = Tbx.DetailsProgress;

	type ProfilesProgress = Tbx.ProfilesProgress;

	type DownloadedMarks = Tbx.DownloadedMarks;

	type PageSettings = Tbx.PageSettings;

	type Json = Tbx.Json;

	type JsonObject = Tbx.JsonObject;

	/** What detectPage makes of the page: its kind, its subject as a path, and what said so. */
	type DetectedPage = { type: PageType; key: string; source: 'dom' | 'url' };

	/**
	 * A track object from SoundCloud's API or the page's hydration, as far as this script reads
	 * one (apiTrackOf): a string it did not send is ''. A playlist's tracks past its first few are
	 * bare `{ id }` stubs.
	 */
	interface ApiTrack {
		/** 'track'; the same answers carry users, playlists and likes. */
		kind: string;
		id?: number;
		permalink_url: string;
		/** False unless SoundCloud said true. */
		downloadable: boolean;
		/** True unless SoundCloud said false. */
		has_downloads_left: boolean;
		purchase_url: string;
		purchase_title: string;
		genre: string;
		artwork_url: string;
		/** The uploader's (`user.avatar_url`). */
		avatar_url: string;
	}

	/** A playlist object, read (apiPlaylistOf). */
	type ApiPlaylist = { permalink_url: string; tracks?: ApiTrack[] };

	/**
	 * A user object from /resolve, which is free to hold anything: each field is checked as read.
	 */
	type ApiUser = JsonObject;

	/**
	 * One entry of the page's `window.__sc_hydration`. `data` is whatever that hydratable is — a
	 * track, a playlist, a user, the API client — and is read according to its name.
	 */
	type HydrationEntry = { hydratable: string; data?: Json };

	/** What an API request asks, besides the client id (which apiGet adds). */
	interface ApiQuery {
		url?: string;
		ids?: string;
		limit?: number;
		offset?: number;
		linked_partitioning?: number;
	}

	/** A layout the markers and the queue button paint (MARKER_TARGETS). */
	interface MarkerTarget {
		/** The layout's rows, tiles, hero or bar. */
		rows: string;
		/** The cover art inside one. */
		art: string;
		/** The link naming its track, where a link does. */
		link?: string;
		/** Else the track, named some other way. */
		key?: () => string;
		variant: 'small' | 'large';
		/** The track as the queue keeps it. */
		fields: (row: ParentNode) => RowFields;
	}

	/** One lookup, queued or running (see "one lookup at a time"). */
	interface Job {
		/** The page its tracks are on. */
		page: { type: PageType; key: string };
		/** That page's tracks. */
		items: { permalink: string; itemIndex?: number }[];
		/** The permalinks it was queued for. */
		wanted: string[];
		wantedSet: Set<string>;
		clientId: string;
		/** Whether SoundCloud turned the client id away. */
		unauthorized: boolean;
		/** Whether it gave up on a page the API cannot see. */
		unseen: boolean;
		/** Settles once it has run; set as it is queued. */
		done?: Promise<void>;
	}

	/** What a request that waited on a lookup is told once its tracks are settled. */
	type LookupOutcome = { unauthorized: boolean; unseen: boolean; joined: boolean };

	/** A request waiting for its tracks to be settled. */
	interface Waiter {
		wanted: string[];
		/** The lookup they were left to. */
		job: Job;
		/** Whether that lookup was already queued when it asked. */
		joined: boolean;
		resolve: (outcome: LookupOutcome) => void;
		/** The permalinks already reported: sent, or settled as failures. */
		told: Set<string>;
	}

	// Track rows on playlist pages. Regular playlists (/user/sets/name) use
	// li.trackList__item; SoundCloud's auto-generated "system" playlists
	// (/discover/sets/...) use li.systemPlaylistTrackList__item. Both wrap the
	// same .trackItem structure. Related-tracks sidebar entries use .soundBadge,
	// not .trackItem, so they are never picked up.
	//
	// Note for Likes pages: a liked playlist embeds a preview of its tracks as
	// ul.trackList__item > li.compactTrackList__item > .trackItem. That wrapper
	// is a UL, not an LI, so 'li.trackList__item .trackItem' cannot match those
	// nested rows either — only the bare '.trackItem' fallback below would, and
	// that fallback is gated to playlist pages.
	const TRACK_ROW_SELECTOR = 'li.trackList__item .trackItem, li.systemPlaylistTrackList__item .trackItem';

	// Liked items on Likes pages, "sound list" layout (public /<user>/likes and
	// /you/likes in List view). Each li.soundList__item wraps one .sound element
	// (via an unclassed div) which is either a track or a playlist. The child
	// combinators keep the selector from descending into a liked playlist's
	// nested track preview, which contains no .sound elements anyway.
	const LIKED_SOUND_SELECTOR = 'li.soundList__item > div > .sound';

	// Liked items on Likes pages, badge grid layout (/you/likes default
	// "Badges" view). Grid filler slots are li.badgeList__item >
	// .audibleTilePlaceholder and are excluded by targeting .audibleTile only.
	const LIKED_TILE_SELECTOR = 'li.badgeList__item > .audibleTile';

	const collapse = (s: string | null | undefined) => (s || '').replace(/\s+/g, ' ').trim();

	// Text of the element's direct text nodes only, skipping child elements.
	// The curator link nests a "Verified" badge whose SVG <title> would leak
	// into textContent; the tile heading nests an icon span.
	const directText = (el: Element | null) => {
		if (!el) return '';
		let out = '';

		for (const node of el.childNodes) {
			if (node.nodeType === Node.TEXT_NODE) out += node.textContent;
		}

		return collapse(out);
	};

	// URL from an element's inline background-image, or '' if none. SoundCloud
	// sets an inline background-image on span.image__full once a thumbnail has
	// lazy-loaded; rows/tiles never scrolled into view have no URL yet (the
	// span's style is just "width: 100%; height: 100%; opacity: 0;") and report
	// '' — this extension never triggers loading itself.
	const backgroundImageUrl = (el: HTMLElement | null) => {
		const match = /url\(["']?(https?:[^"')]+)["']?\)/.exec(el?.style.backgroundImage || '');

		return match ? match[1] : '';
	};

	// Thumbnail artwork of an element, found via a selector scoped to it.
	const artworkUrlOf = (scope: ParentNode, selector: string) => backgroundImageUrl(scope.querySelector(selector));

	// Collapsed text of the first element a selector finds inside another, or
	// '' where the layout has no such element (the player bar names no artist).
	const textOf = (scope: ParentNode, selector: string) => collapse(scope.querySelector(selector)?.textContent);

	// Whether a link's href points at a playlist (/user/sets/name).
	const isPlaylistLink = (a: Element | null) => (a?.getAttribute('href') || '').includes('/sets/');

	// Path of a URL (absolute, or relative to this origin) without a trailing
	// slash, e.g. "/user/track"; '' if unparsable. A track's permalink path is
	// its identity throughout this extension. SoundCloud appends
	// "?in=<user>/sets/<playlist>" to title links on playlist pages; dropping
	// the query makes the same track match on every page.
	// A link's href as a URL, resolved against this page, or null where there is
	// no reading it as one. Only one reader needs the whole URL — the signed-in
	// layout's, which has to tell a link to this site from a link off it — but
	// the parse lives here so there is one place that can fail.
	const urlOf = (url: string | null | undefined) => {
		if (!url) return null;

		try {
			return new URL(url, location.origin);
		} catch (_e) {
			return null;
		}
	};

	const pathOf = (url: string | null | undefined) => urlOf(url)?.pathname.replace(/\/+$/, '') || '';

	const permalinkPathOf = (a: Element | null | undefined) => pathOf(a?.getAttribute('href'));

	// Whether one of SoundCloud's own toggle buttons in a row is on — its like
	// button, its repost button. Those say so twice, in the class list and in
	// `aria-pressed`, and either is enough: a redesign that drops one of them
	// still reads correctly through the other.
	//
	// `null`, not `false`, where the row renders no such button at all (the
	// badge tiles render none, and a signed-out page renders them unpressed for
	// nobody in particular). "Not liked" is a claim about the signed-in user,
	// and a row with no button to read is not making it — so the panel can say
	// "unknown" rather than tell somebody their own like is not there.
	const pressedState = (row: ParentNode, selector: string) => {
		const button = row.querySelector(selector);

		if (!button) return null;

		return button.classList.contains('sc-button-selected') || button.getAttribute('aria-pressed') === 'true';
	};

	// A row's like and repost state, read the same way in every layout.
	const engagementOf = (row: ParentNode): Pick<PageFacts, 'liked' | 'reposted'> => ({
		liked: pressedState(row, '.sc-button-like'),
		reposted: pressedState(row, '.sc-button-repost')
	});

	// The genre tag SoundCloud renders beside a row's title, and on a track
	// page's hero: one `.sc-tag` holding the uploader's genre — "Dance & EDM",
	// or whatever they typed ("bolha"). It is the track's `genre` field and not
	// one of its free tags, which SoundCloud renders nowhere in a list; both
	// were read off soundcloud.com and checked against the same tracks' own
	// page data in September 2026.
	//
	// Reading it here is what makes the genre a fact about the page rather than
	// an answer to be fetched: a Likes page names the genre of every row it
	// draws, and asking SoundCloud what it has already said is a request for
	// nothing. Layouts that render no tag — playlist rows, the badge tiles —
	// give `null`, which is "the page never said", not "no genre set" ('').
	const GENRE_TAG = '.soundTitle__tagContainer .sc-tagContent';

	const genreOf = (row: ParentNode, selector = GENRE_TAG) => {
		const tag = row.querySelector(selector);

		return tag ? collapse(tag.textContent) : null;
	};

	// SoundCloud wraps outbound links in its gate.sc redirector
	// (https://gate.sc?url=<encoded>&token=…); the real destination is the
	// `url` parameter. Returns '' for no link.
	const unwrapGate = (href: string | null | undefined) => {
		if (!href) return '';

		try {
			const url = new URL(href, location.origin);

			if (url.hostname === 'gate.sc') return url.searchParams.get('url') || '';

			return url.href;
		} catch (_e) {
			return '';
		}
	};

	// ---- Which page is this? ---------------------------------------------------
	//
	// SoundCloud is a single-page app, and its URL and its content do not
	// always agree: the address bar can show one page while another is still
	// (or already) rendered, and some in-app links change the URL without ever
	// rendering the page — /you/insights has been seen with the Likes page on
	// screen. The tools work on what is rendered, so the page type is read from
	// the DOM, where each kind of page has a layout of its own; the URL is only
	// consulted when nothing recognisable is rendered (yet). Row selectors
	// alone never decide: profile pages embed the same .sound rows and
	// .trackItem previews as Likes and playlist pages.
	//
	//   playlist  a "listen" page whose hero carries the tracks summary
	//             (.fullHero__tracksSummary — regular playlists and albums under
	//             .l-listen-hero, system playlists as .systemPlaylistHero), or
	//             whose track list is rendered (.listenDetails .trackList,
	//             .systemPlaylistDetails). Which playlist: the
	//             "?in=<user>/sets/<name>" (regular) or "?in_system_playlist=<id>"
	//             (system, i.e. /discover/sets/<id>) query SoundCloud puts on
	//             every row's title link.
	//   track     a "listen" page without those: the hero, and the engagement
	//             bar (.listenEngagement) below it. Which track: the URL — the
	//             hero names the uploader, never the track's own path — but
	//             only when the URL looks like a track path under that uploader.
	//   likes     a tab bar whose active tab (a.g-tabs-link.active) links to
	//             /<user>/likes or /you/likes. On public /<user>/likes that is
	//             li.networkTabs__likes in ul.userNetworkTabs, next to
	//             h1.userNetworkTop__title ("Likes by <User>"). Failing that,
	//             the Library layout of the signed-in user's own likes —
	//             .collectionSection with the Badges/List view toggle
	//             (.listDisplayToggle) — counts as /you/likes. Whose likes:
	//             'you', or the profile permalink from the tab's href.
	//   other     anything else (profile, stream, search, ...).

	const LIKES_PATH = /^\/[^/]+\/likes$/;

	// What the URL alone says: 'playlist' | 'likes' | 'other'.
	const pageTypeOfUrl = (pathname: string) => {
		if (pathname.includes('/sets/')) return 'playlist';

		// Both the signed-in user's own likes (/you/likes) and any user's public
		// likes (/<user>/likes).
		if (LIKES_PATH.test(pathname.replace(/\/+$/, ''))) return 'likes';

		return 'other';
	};

	// The playlist the rendered rows belong to, as a path, from the query on
	// their title links; '' when no row carries one.
	const playlistPathOfRows = () => {
		for (const row of document.querySelectorAll(TRACK_ROW_SELECTOR)) {
			const href = row.querySelector('.trackItem__trackTitle')?.getAttribute('href');

			if (!href) continue;
			let params;

			try {
				params = new URL(href, location.origin).searchParams;
			} catch (_e) {
				continue;
			}

			const set = params.get('in');

			if (set) return `/${set.replace(/^\/+/, '')}`;
			const system = params.get('in_system_playlist');

			if (system) return `/discover/sets/${system}`;
		}

		return '';
	};

	// The track a track page shows, as a path, or '' when unsure: the URL's
	// path, accepted only when it has the shape of a track path (/<user>/<track>)
	// whose user is the uploader the hero names — so a URL that disagrees with
	// the content names no track rather than the wrong one.
	const trackPathOfPage = () => {
		const path = pathOf(location.pathname);

		if (!/^\/[^/]+\/[^/]+$/.test(path)) return '';
		const uploader = pathOf(document.querySelector('.fullHero .soundTitle__username a')?.getAttribute('href'));

		if (uploader && uploader.split('/')[1] !== path.split('/')[1]) return '';

		return path;
	};

	// ---- The signed-in track page, which is a page inside a page -------------
	//
	// Signed in, SoundCloud no longer renders a track page the way the rest of
	// this file reads one. The tab's own document keeps the header and the
	// player bar, and the track itself — hero, actions, comments — arrives in
	// an iframe of its own: `iframe.webiIframe` loading
	// `soundcloud.com/n/<user>/<track>?v2_layout=true`, a React app whose
	// markup shares nothing with the classic page. There is no `.fullHero`, no
	// `.listenEngagement`, no `.sc-button` — the buttons are MUI's, and the
	// class that would name one (`mui-163v5oj`) is a hash of the stylesheet
	// that built it, so it is no more nameable than a minified variable.
	//
	// What that frame *does* carry is the accessibility tree, which is the part
	// SoundCloud cannot rename without breaking a screen reader: a
	// `section[aria-label="Track header"]` around the hero, an `<h1>` for the
	// title, and one outbound link in that header — the purchase link, the
	// uploader's "Buy"/"Download", drawn as a cart. That link is what this
	// extension replaces everywhere else, and it is the one the setting is
	// about, so it is replaced here too.
	//
	// The script reaches the frame at all because the manifest says
	// `all_frames`. See the message listener at the end of this file for the
	// one thing that stays in the tab's own document.

	const V2_HEADER = 'section[aria-label="Track header"]';

	// The frame's path is the page's with `/n` in front of it, which is the
	// whole of the difference: `/n/<user>/<track>` is the track at
	// `/<user>/<track>`, and that is the permalink everything downstream keeps.
	const v2TrackPathOfPage = (header: ParentNode) => {
		const path = pathOf(location.pathname).replace(/^\/n\//, '/');

		if (!/^\/[^/]+\/[^/]+$/.test(path)) return '';
		const uploader = pathOf(v2UploaderLink(header)?.getAttribute('href'));

		if (uploader && uploader.split('/')[1] !== path.split('/')[1]) return '';

		return path;
	};

	// The uploader, among the handful of links the header holds: the one that
	// goes to a profile on this site. The genre link goes to /tags/<genre>, and
	// the purchase link goes somewhere else entirely — which is why the origin
	// is checked and not only the path. `pathOf` resolves a link against this
	// page, so an outbound `https://hypeddit.com/x` reads as `/x` and looks
	// exactly like a profile; so does this extension's own button, which is put
	// in that row and holds the same outbound link. Reading either as the
	// uploader would put "Download" in the queue where the artist goes.
	//
	// A profile is one segment, which a genre's /tags/<genre> is not. The tags
	// index itself is one segment too, so that one path is named — and only
	// that one: an account whose name merely starts with "tags" is somebody's
	// profile like any other.
	const v2UploaderLink = (header: ParentNode) => {
		for (const link of header.querySelectorAll('a[href]')) {
			const url = urlOf(link.getAttribute('href'));

			if (!url || url.origin !== location.origin) continue;
			const path = url.pathname.replace(/\/+$/, '');

			if (/^\/[^/]+$/.test(path) && path !== '/tags') return link;
		}

		return null;
	};

	// The track the header describes, as the queue keeps it. The artwork is an
	// <img> here rather than a background image, so it is read from `src`
	// rather than through artworkUrlOf; the title is taken from the `title`
	// attribute first, which holds the whole of a name the heading itself may
	// have had to cut.
	const v2FieldsOf = (header: ParentNode): RowFields => {
		const heading = header.querySelector('h1');
		const art = [...header.querySelectorAll('img[src]')].find((img) => img.getAttribute('src')!.includes('artworks-'));
		const tag = header.querySelector('a[href*="/tags/"]');

		return {
			title: collapse(heading?.getAttribute('title')) || collapse(heading?.textContent),
			artist: collapse(v2UploaderLink(header)?.textContent),
			artworkUrl: art?.getAttribute('src') || '',
			genre: tag ? collapse(tag.textContent) : null,
			// Whether this account has liked or reposted the track is drawn here as
			// the word on the button ("Like" against "Unlike") and nowhere else —
			// no pressed state, no class of its own. That is SoundCloud's English,
			// not a fact about the track, so this layout says nothing rather than
			// reading a label back: "nobody said", which is what a tile says too.
			liked: null,
			reposted: null
		};
	};

	// { type: 'playlist' | 'likes' | 'track' | 'other', key, source }. `key` is
	// the page's subject as a path — the playlist, the Likes page (/<user>/likes
	// or /you/likes), the track ('' when unsure) — and '' for other pages;
	// `source` is 'dom', or 'url' when nothing recognisable was rendered and
	// the URL had to do.
	const detectPage = (): DetectedPage => {
		if (
			document.querySelector('.fullHero .fullHero__tracksSummary, .listenDetails .trackList, .systemPlaylistDetails')
		) {
			return {
				type: 'playlist',
				key: playlistPathOfRows() || pathOf(location.pathname),
				source: 'dom'
			};
		}

		if (document.querySelector('.l-listen-hero .fullHero, .l-listen-wrapper .listenEngagement')) {
			return { type: 'track', key: trackPathOfPage(), source: 'dom' };
		}

		// The same page, drawn for a signed-in account inside its own frame.
		const v2Header = document.querySelector(V2_HEADER);

		if (v2Header) {
			return { type: 'track', key: v2TrackPathOfPage(v2Header), source: 'dom' };
		}

		for (const tab of document.querySelectorAll('a.g-tabs-link.active')) {
			const path = pathOf(tab.getAttribute('href'));

			if (LIKES_PATH.test(path)) return { type: 'likes', key: path, source: 'dom' };
		}

		if (document.querySelector('.collectionSection .listDisplayToggle')) {
			return { type: 'likes', key: '/you/likes', source: 'dom' };
		}

		const type = pageTypeOfUrl(location.pathname);

		return { type, key: type === 'other' ? '' : pathOf(location.pathname), source: 'url' };
	};

	// Whose likes a Likes page lists: 'you', or the profile permalink. (A
	// Likes page's key is always "/<owner>/likes", so the fallback is only
	// for safety — the coverage hint marks it as unreachable.)
	const likesOwner = (page: { key: string }) => /* v8 ignore next */ page.key.split('/')[1] || '';

	// ---- Reading a row's fields ----------------------------------------------
	//
	// A track is described twice on every page: once when the popup asks for
	// the whole list (the extractors below), and once when the "+ queue" button
	// on its cover art is clicked and that one track has to be named. Both read
	// the same elements, so they read them through the same few functions —
	// otherwise a track queued from the page could carry a different title from
	// the same track queued from the panel.

	// Playlist rows (.trackItem).
	const ROW_TITLE = '.trackItem__trackTitle';
	const ROW_ARTIST = '.trackItem__username';
	const ROW_ART = '.trackItem__image span.image__full';

	// "Sound list" rows (.sound) on Likes pages.
	const SOUND_TITLE = '.soundTitle__title';
	const SOUND_ART = '.sound__coverArt .image__full';

	// Badge grid tiles (.audibleTile) on Likes pages.
	const TILE_ARTIST = '.playableTile__usernameHeading';
	const TILE_ART = '.playableTile__image .image__full';

	// .soundTitle__usernameText holds the display artist (e.g. "BARELY ALIVE"),
	// which may differ from the uploader account in the href; the surrounding
	// link is the fallback should the inner span disappear.
	const soundArtistOf = (row: ParentNode) =>
		collapse(
			(row.querySelector('.soundTitle__usernameText') || row.querySelector('.soundTitle__username'))?.textContent
		);

	// A tile's heading anchor holds the title as direct text plus an icon span;
	// fall back to the full text should the markup change.
	const tileTitleOf = (tile: ParentNode) => {
		const heading = tile.querySelector('.playableTile__mainHeading');

		return directText(heading) || collapse(heading?.textContent);
	};

	// A track page's hero, which is the only thing on such a page that names
	// its track: there are no rows to read. Two things go through it — the
	// marker on the hero artwork, and the replaced download buttons in the
	// action bar below it, which sit outside every row selector there is.
	const HERO_ROWS = '.l-listen-hero .fullHero';

	// The hero's own genre tag sits in the title block rather than a row's tag
	// container, and the like and repost buttons are not in the hero at all:
	// they are in the engagement bar below it, which is where a track page puts
	// what a list puts in each row. Both are read from where the page has them,
	// so a track queued from a track page carries what one queued from a Likes
	// row does.
	const HERO_TAG = '.sc-tagContent';
	const HERO_ENGAGEMENT = '.listenEngagement';

	const heroFieldsOf = (hero: ParentNode) => ({
		title: textOf(hero, SOUND_TITLE),
		// The uploader link, read exactly as a playlist hero's curator is: direct
		// text nodes only, because the link nests a "Verified" badge whose SVG
		// <title> is text and would otherwise be part of the name — "kstko
		// Verified", read off a live track page.
		artist: directText(hero.querySelector('.soundTitle__username a')),
		artworkUrl: artworkUrlOf(hero, '.listenArtworkWrapper__artwork .image__full'),
		genre: genreOf(hero, HERO_TAG),
		...engagementOf(document.querySelector(HERO_ENGAGEMENT) || hero)
	});

	// ---- Track pages ---------------------------------------------------------

	// The one track a track page is about, as a list of the one — so a page
	// with a single song answers in the shape a page with fifty does and
	// everything downstream (the count, the lookup, the Manager's list) works on
	// it without knowing which kind of page it came off.
	//
	// There are no rows here. The hero names the track and the engagement bar
	// below it carries what a row's actions would — the like and repost
	// buttons, and the purchase link the uploader set — so a track added from
	// its own page carries what the same track added from a Likes row does.
	//
	// `page.key` is the permalink: the URL, checked against the hero (see
	// detectPage). It is '' when the two disagree, and the track is still
	// reported then — a Likes row whose title is not a link is reported the
	// same way, and it is the list, not the reader, that decides a track with
	// no identity cannot be held.
	const extractHeroTrack = (page: DetectedPage): PageTrack[] => {
		const hero = document.querySelector(HERO_ROWS);

		if (!hero) return [];
		const fields = heroFieldsOf(hero);

		if (!fields.artist && !fields.title) return [];

		// A track page renders the purchase link in the engagement bar rather
		// than in a row's actions; the legacy class is checked too, as the click
		// listener below checks it. The label is in `aria-label` where SoundCloud
		// sets one and is the link's own text otherwise.
		const purchase = document.querySelector(
			`${HERO_ENGAGEMENT} a.soundActions__purchaseLink, ${HERO_ENGAGEMENT} a.sc-buylink`
		);

		return [
			{
				...fields,
				permalink: page.key,
				itemIndex: 0,
				domPurchaseUrl: unwrapGate(purchase?.getAttribute('href')),
				domPurchaseTitle: collapse(purchase?.getAttribute('aria-label')) || collapse(purchase?.textContent)
			}
		];
	};

	// ---- Playlist pages ------------------------------------------------------

	const extractPlaylistTracks = (isPlaylistPage: boolean): PageTrack[] => {
		let rows = document.querySelectorAll(TRACK_ROW_SELECTOR);

		if (rows.length === 0 && isPlaylistPage) {
			// Fallback in case SoundCloud renames the <li> wrapper classes but
			// keeps the .trackItem structure. Gated to playlist pages: profile,
			// stream and Likes pages embed playlist previews whose .trackItem rows
			// are not the page's own tracks.
			rows = document.querySelectorAll('.trackItem');
		}

		const tracks = [];

		for (const row of rows) {
			const titleLink = row.querySelector(ROW_TITLE);
			const artist = textOf(row, ROW_ARTIST);
			const title = collapse(titleLink?.textContent);

			if (artist || title) {
				tracks.push({
					artist,
					title,
					artworkUrl: artworkUrlOf(row, ROW_ART),
					permalink: permalinkPathOf(titleLink),
					// Playlist rows render no tag of their own, so this is null here
					// and the genre comes from the page's hydration (free) or a lookup.
					genre: genreOf(row),
					...engagementOf(row)
				});
			}
		}

		return tracks;
	};

	// Playlist metadata lives in the hero banner. Scope to .fullHero so the
	// sidebar's playlist cards (which reuse .soundTitle__title) are ignored.
	// Regular playlists use .soundTitle__*; system playlists (/discover/sets/)
	// use .fullHero__titleText*.
	// No hero banner (e.g. the stream or a profile page) means no playlist
	// metadata — leave the fields empty rather than scraping unrelated cards.
	const playlistMetadata = () => {
		const hero = document.querySelector('.fullHero');

		if (!hero) return { title: '', curator: '' };

		return {
			title: collapse(hero.querySelector('.soundTitle__title, .fullHero__titleTextTitle')?.textContent),
			curator: directText(hero.querySelector('.soundTitle__username a, .fullHero__titleTextLineSmall a'))
		};
	};

	// ---- Likes pages ---------------------------------------------------------

	// Returns { tracks, skippedPlaylists }. Liked playlists are skipped (and
	// counted) rather than expanded: their embedded track previews are partial
	// and the page's subject is the liked items, not their contents.
	//
	// Only one of the two layouts is rendered at a time, but both are scanned so
	// whichever is present is picked up; each selector is anchored on its own
	// list-item class so neither can match the other's markup.
	//
	// Each track also records `itemIndex`, its position among all liked items
	// (playlists included), which is the position in SoundCloud's likes
	// listing too — the Manager tool uses it to ask for exactly as many
	// likes as the page shows. Rows in layout A render the purchase link
	// ("Buy" / "Free Download") as well; it is read into `domPurchaseUrl` /
	// `domPurchaseTitle` so nothing needs to be asked for it. Every layout also
	// reports `liked` / `reposted` from the row's own buttons — page state, not
	// track metadata, so it is never cached with the details — and `genre`,
	// which layout A's rows carry as a tag and layout B's tiles do not.
	const extractLikedTracks = () => {
		const tracks: PageTrack[] = [];
		let skippedPlaylists = 0;
		let itemIndex = 0;

		const push = (fields: PageTrack) => {
			if (fields.artist || fields.title) tracks.push(fields);
		};

		// Layout A — "sound list" rows. A liked playlist carries the "playlist"
		// class; the title link's href (/user/sets/name) is checked as well in
		// case that class marker goes away. Fields are read from the row's own
		// .soundTitle block, which sits above the nested playlist preview.
		for (const row of document.querySelectorAll(LIKED_SOUND_SELECTOR)) {
			const index = itemIndex++;
			const titleLink = row.querySelector(SOUND_TITLE);

			if (row.classList.contains('playlist') || isPlaylistLink(titleLink)) {
				skippedPlaylists++;
				continue;
			}

			// The purchase link, when the uploader set one: an <a> wrapped in
			// gate.sc whose aria-label is SoundCloud's label for it.
			const purchase = row.querySelector('a.soundActions__purchaseLink');
			push({
				artist: soundArtistOf(row),
				title: collapse(titleLink?.textContent),
				artworkUrl: artworkUrlOf(row, SOUND_ART),
				permalink: permalinkPathOf(titleLink),
				itemIndex: index,
				domPurchaseUrl: unwrapGate(purchase?.getAttribute('href')),
				domPurchaseTitle: collapse(purchase?.getAttribute('aria-label')),
				// The row says what the uploader filed it under, so nothing has to be
				// asked for it: a Likes page answers the genre of every row it draws.
				genre: genreOf(row),
				...engagementOf(row)
			});
		}

		// Layout B — badge grid tiles. Track and playlist tiles share exactly the
		// same classes (audibleTile playableTile) — there is NO class marker — so
		// the only tell is a /sets/ segment in the heading or artwork link href.
		// Tiles render no purchase link.
		for (const tile of document.querySelectorAll(LIKED_TILE_SELECTOR)) {
			const index = itemIndex++;
			const heading = tile.querySelector('.playableTile__mainHeading');
			const artworkLink = tile.querySelector('.playableTile__artworkLink');

			if (isPlaylistLink(heading) || isPlaylistLink(artworkLink)) {
				skippedPlaylists++;
				continue;
			}

			push({
				artist: textOf(tile, TILE_ARTIST),
				title: tileTitleOf(tile),
				artworkUrl: artworkUrlOf(tile, TILE_ART),
				permalink: permalinkPathOf(heading) || permalinkPathOf(artworkLink),
				itemIndex: index,
				domPurchaseUrl: '',
				domPurchaseTitle: '',
				// A tile renders no tag either, so the genre is unknown here too.
				genre: genreOf(tile),
				// Tiles render no repost button, and their like button only where
				// SoundCloud offers the action, so either can come back null.
				...engagementOf(tile)
			});
		}

		return { tracks, skippedPlaylists };
	};

	// Likes pages have no .fullHero. The public page has a heading
	// ("Likes by <User>") in h1.userNetworkTop__title; the signed-in user's own
	// /you/likes has no such heading, so a fixed name is used.
	const likesTitle = (page: DetectedPage) => {
		const heading = collapse(document.querySelector('h1.userNetworkTop__title')?.textContent);

		if (heading) return heading;

		return likesOwner(page) === 'you' ? 'your likes' : '';
	};

	// ---- Extraction entry point ----------------------------------------------

	const extract = () => {
		const page = detectPage();

		const result: TracksAnswer = {
			ok: true,
			url: location.href,
			// 'playlist' | 'likes' | 'track' | 'other'.
			pageType: page.type,
			// The playlist, Likes page or track the tracks belong to, as a path;
			// '' on anything else (and on a track page whose track is not named).
			pageKey: page.type === 'other' ? '' : page.key,
			pageDetectedBy: page.source,
			title: '',
			curator: '',
			tracks: [],
			skippedPlaylists: 0
		};

		if (page.type === 'likes') {
			const { tracks, skippedPlaylists } = extractLikedTracks();
			result.title = likesTitle(page);
			result.tracks = tracks;
			result.skippedPlaylists = skippedPlaylists;
		} else {
			// Playlist pages, track pages, plus other pages — all three read the
			// same hero for their title and uploader. Other pages get the
			// (non-fallback) row scan as well, so a changed layout still has a
			// chance of working.
			const { title, curator } = playlistMetadata();
			result.title = title;
			result.curator = curator;
			result.tracks = page.type === 'track' ? extractHeroTrack(page) : extractPlaylistTracks(page.type === 'playlist');
		}

		return result;
	};

	// ---- Download details ----------------------------------------------------
	//
	// Whether a track can be downloaded is not visible in playlist or Likes
	// rows: SoundCloud's own "Download file" control sits in each row's More
	// menu, which only renders when opened, and playlist rows show no purchase
	// link at all. Everything the page already holds is used first; SoundCloud
	// is asked only for what is left — in bulk, anonymously, read-only — and
	// the answers are kept for a month so nothing is asked twice.
	//
	// Free sources (no request):
	//   1. The page's hydration data. SoundCloud server-renders a <script>
	//      element containing `window.__sc_hydration = [...]`, the JSON the
	//      page was built from. Being a DOM element, its text can be read here
	//      (parsed as JSON, never executed). It holds full track objects for a
	//      track page's track and for the first few tracks of a playlist, bare
	//      {id} stubs for the playlist's other tracks, and on every page the
	//      public API client id the page itself uses. Every full track in it
	//      is harvested into the cache when the page loads. It describes the
	//      page loaded by the last full navigation, so after in-app navigation
	//      it is stale (the client id in it stays valid).
	//   2. Likes rows (List view) render the purchase link and its label; see
	//      extractLikedTracks. That settles purchase links for those rows;
	//      only "downloadable" is unknown.
	//
	// Requests — only when the panel asks, only for tracks the cache does not
	// know, all to SoundCloud's public API (api-v2.soundcloud.com, the one the
	// page calls), anonymous (that client id, no cookies, no auth header), GET
	// only, public track metadata:
	//   - playlist pages: /tracks?ids= for the hydration's stubs, 50 per
	//     request, in playlist order, only as far as the rendered rows need.
	//     After in-app navigation, one /resolve of the playlist URL first, for
	//     its track ids (half the bytes of re-fetching the page, and no
	//     server-side render).
	//   - Likes pages: the likes listing (/users/<id>/likes) the page itself
	//     loads, in page order, sized to the rendered rows and at most 200 per
	//     request — one request instead of one per track. The user id comes
	//     from the hydration (one /resolve of the profile URL after in-app
	//     navigation).
	//   - track pages: nothing of their own. The one track is in the page's
	//     own hydration, so a track page loaded fresh is answered without a
	//     request at all; after in-app navigation it falls to the resolve
	//     below, which is one request for one track.
	//   - whatever those did not cover (private tracks, a filtered Likes list,
	//     rows past the listing's reach): /resolve?url= per track, two at a
	//     time. Failures are remembered for a day so they are not retried on
	//     every panel open.
	// The API accepts cross-origin calls from soundcloud.com only, which is why
	// the content script makes them (from the page's origin) rather than the
	// panel. 429 answers are backed off and retried a couple of times.
	//
	// Everything learned goes into the cache as it arrives, not when a lookup
	// ends, and is written back shortly after — so a lookup cut short by a
	// reload keeps what it had, markers appear as answers come in, and a later
	// request benefits from an earlier one's progress. Lookups themselves run
	// one at a time; see "one lookup at a time" below.

	const API_ORIGIN = 'https://api-v2.soundcloud.com';
	const API_BATCH = 50; // ids per /tracks request
	// How many requests of one kind are in the air at once. Every request here
	// costs a round trip to SoundCloud rather than any work worth speaking of,
	// so what a lookup takes is how many of them it waits through end to end:
	// one at a time, five hundred tracks is ten round trips of half a second to
	// two seconds each — ten seconds of a progress bar for a list the panel
	// could otherwise have drawn in two. Four at a time was measured against
	// the live API at a quarter to a fifth of the wall clock and not one `429`;
	// six was no better, the server being the queue by then. This is the number
	// the extension is polite at, not the number it is fastest at.
	const API_CONCURRENCY = 4;
	// How many rows are resolved one at a time before the lookup decides the
	// answer is always going to be no — see "nothing to be seen" in runJob.
	const PROBE = 5;
	const LIKES_PAGE = 200; // likes per listing request (the API's maximum)
	const LIKES_MAX_PAGES = 10; // how deep the listing is followed (2,000 likes)
	const HYDRATION_PREFIX = 'window.__sc_hydration = ';
	const DETAILS_KEY = 'details';
	const DETAILS_TTL_MS = 30 * 24 * 60 * 60 * 1000; // a track's details
	const FAILURE_TTL_MS = 24 * 60 * 60 * 1000; // "could not be looked up"
	const SAVE_DELAY_MS = 1000; // how soon after learning something the cache is written back

	// How many tracks the cache keeps. Above this the oldest are dropped.
	//
	// This is not tidiness. chrome.storage.local is one 10 MB area and nothing
	// waits on a write to it, so a cache that outgrew the quota would fail its
	// write silently and go on failing — and a cache that cannot be written is
	// a cache that is empty on the next visit, which sends every track on every
	// page back to SoundCloud, forever. The failure mode of the cap is a handful
	// of extra requests for pages not seen in weeks; the failure mode of having
	// no cap is all of them. At roughly 290 bytes an entry this is about 6 MB,
	// which leaves the marks (downloaded-store.js) room beside it.
	const CACHE_MAX = 20000;

	const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

	/**
	 * Runs `task` over `items`, at most `limit` of them in the air at once,
	 * taking the next item as each worker frees up (rather than in fixed
	 * rounds, so one slow answer does not hold the others).
	 *
	 * `keepGoing` is checked before each item is picked up — both callers have
	 * one, so it is required rather than defaulted: a lookup stops as soon as
	 * it has what the rendered rows asked for, and the requests already out are
	 * simply the ones it had to make to find that out.
	 *
	 * A task that throws rethrows here at once — the caller has somewhere else
	 * to go (a playlist batch that failed is resolved track by track instead)
	 * and waiting for the other requests would only delay it. Those keep their
	 * place: they take no new item, and whatever they still learn is kept. Each
	 * worker catches for itself so that a straggler failing after the pool has
	 * been left behind is never an unhandled rejection.
	 */
	const pool = <T>(items: T[], limit: number, task: (item: T) => Promise<void>, keepGoing: () => boolean) => {
		let next = 0;
		let failed = false;
		let fail: (cause: unknown) => void;

		const failure = new Promise((_resolve, reject) => {
			fail = reject;
		});

		const worker = async () => {
			while (next < items.length && !failed && keepGoing()) {
				try {
					await task(items[next++]);
				} catch (e) {
					failed = true;
					fail(e);
				}
			}
		};

		const workers = Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
		// Whichever of the two loses the race is settled and never looked at
		// again; a rejection nobody is waiting for is one the browser reports.
		failure.catch(() => {});

		return Promise.race([workers, failure]);
	};

	class ApiError extends Error {
		declare status: number;

		/** @param status the HTTP status SoundCloud answered with. */
		constructor(status: number) {
			super(`HTTP ${status}`);
			this.status = status;
		}
	}

	// ---- reading what SoundCloud sends
	//
	// The hydration and the API answer in JSON (chrome.storage keeps it too),
	// and nothing past the readers below asks what kind of value something is: each takes whatever came and
	// answers with what it reads, or a stand-in for nothing ('', {}, []).

	const isString = (value: Json | undefined): value is string => typeof value === 'string';
	const stringOf = (value: Json | undefined) => (isString(value) ? value : '');

	const isObject = (value: Json | undefined): value is JsonObject =>
		typeof value === 'object' && value !== null && !Array.isArray(value);

	const objectOf = (value: Json | undefined): JsonObject => (isObject(value) ? value : {});
	const listOf = (value: Json | undefined) => (Array.isArray(value) ? value : []);
	// (Number() of a number is that number.)
	const integerOf = (value: Json | undefined) => (Number.isInteger(value) ? Number(value) : undefined);
	const integerOrNull = (value: Json | undefined) => integerOf(value) ?? null;

	// A track object, as ApiTrack reads it. Anything else reads as a track of
	// nothing, which every caller passes over.
	const apiTrackOf = (value: Json | undefined): ApiTrack => {
		const track = objectOf(value);

		return {
			kind: stringOf(track.kind),
			id: integerOf(track.id),
			permalink_url: stringOf(track.permalink_url),
			downloadable: track.downloadable === true,
			has_downloads_left: track.has_downloads_left !== false,
			purchase_url: stringOf(track.purchase_url),
			purchase_title: stringOf(track.purchase_title),
			genre: stringOf(track.genre),
			artwork_url: stringOf(track.artwork_url),
			avatar_url: stringOf(objectOf(track.user).avatar_url)
		};
	};

	const apiPlaylistOf = (value: Json | undefined): ApiPlaylist => {
		const playlist = objectOf(value);

		return {
			permalink_url: stringOf(playlist.permalink_url),
			tracks: Array.isArray(playlist.tracks) ? playlist.tracks.map(apiTrackOf) : undefined
		};
	};

	const hydrationEntryOf = (value: Json): HydrationEntry => {
		const entry = objectOf(value);

		return { hydratable: stringOf(entry.hydratable), data: entry.data };
	};

	// The hydration array from script text holding
	// "window.__sc_hydration = [...];", or null.
	const parseHydration = (text: string): HydrationEntry[] | null => {
		// Only ever called with script text that contains the prefix (see
		// domHydration); the two guards are for safety, hence the coverage hints.
		const start = /* v8 ignore next */ (text || '').indexOf(HYDRATION_PREFIX);

		/* v8 ignore if */
		if (start < 0) return null;
		let json = text.slice(start + HYDRATION_PREFIX.length);
		const end = json.indexOf('</script>');

		if (end >= 0) json = json.slice(0, end);

		try {
			const parsed: Json = JSON.parse(json.trim().replace(/;$/, ''));

			return Array.isArray(parsed) ? parsed.map(hydrationEntryOf) : null;
		} catch (_e) {
			return null;
		}
	};

	const hydratable = (hydration: HydrationEntry[] | null, name: string) =>
		(hydration || []).find((entry) => entry.hydratable === name)?.data;

	// Hydration from the <script> in the DOM, parsed once per document (the
	// script never changes; it is stale after in-app navigation).
	/** Undefined until the script has been looked for. */
	let hydrationMemo: HydrationEntry[] | null | undefined;

	const domHydration = () => {
		if (hydrationMemo !== undefined) return hydrationMemo;
		hydrationMemo = null;

		for (const script of document.scripts) {
			if (!script.src && script.textContent.includes(HYDRATION_PREFIX)) {
				hydrationMemo = parseHydration(script.textContent);
				break;
			}
		}

		return hydrationMemo;
	};

	// The client id the page's own API client uses, or '' where the hydration
	// names none.
	const pageClientId = () => stringOf(objectOf(hydratable(domHydration(), 'apiClient')).id);

	// GET from the API: `url` is a path or a full URL (the listing's
	// next_href). Anonymous — no cookies or auth header ride along.
	/** @returns the answer's JSON, which is whatever SoundCloud sent. */
	const apiGet = async (url: string, params: ApiQuery, clientId: string): Promise<Json> => {
		const target = new URL(url, API_ORIGIN);

		for (const [key, value] of Object.entries(params)) {
			// A number goes in as its digits.
			target.searchParams.set(key, String(value));
		}

		target.searchParams.set('client_id', clientId);

		for (let attempt = 0; ; attempt++) {
			const response = await fetch(target, { credentials: 'omit' });

			if (response.ok) return response.json();

			// Rate limited: back off briefly and retry a couple of times.
			if (response.status === 429 && attempt < 2) {
				await sleep(1500 * (attempt + 1));
				continue;
			}

			throw new ApiError(response.status);
		}
	};

	// Answers to /resolve, kept for the life of the document.
	//
	// Both callers ask only when the hydration cannot say — after in-app
	// navigation, when the script in the DOM still describes the page before it —
	// and both ask for something that does not change while the page is open: a
	// playlist's track ids, a user's id. Without this each panel opened on such a
	// page spends a request learning the same thing again, which is a request
	// per opening for nothing.
	//
	// The promise is what is kept, so two lookups asking at once make one
	// request. A failed one is dropped rather than remembered: the next lookup
	// should be able to try again.
	/** 'playlist:<path>' or 'user:<name>' -> the request, which answers what apiGet does. */
	const resolved: Map<string, Promise<Json>> = new Map();

	const resolveOnce = (key: string, get: () => Promise<Json>) => {
		if (!resolved.has(key)) {
			resolved.set(
				key,
				get().catch((e) => {
					resolved.delete(key);
					throw e;
				})
			);
		}

		return resolved.get(key)!;
	};

	// The cover an API track object names: its own artwork, else the uploader's
	// avatar — which is exactly what SoundCloud's own rows show for a track that
	// has no artwork of its own, so it is the same picture the page would have
	// rendered. The API answers with the legacy `-large` size token (100 px),
	// which lib/artwork.js rewrites like any other, so a click on it still gets
	// 1080 × 1080.
	const apiArtworkUrl = (track: ApiTrack) => track.artwork_url || track.avatar_url;

	// The subset of an API track object this extension keeps.
	/** @returns what the cache files, before `learn` stamps it. */
	const detailsOf = (track: ApiTrack): Omit<KnownDetails, 'at'> => ({
		id: track.id,
		permalink: pathOf(track.permalink_url),
		downloadable: track.downloadable,
		hasDownloadsLeft: track.has_downloads_left,
		purchaseUrl: track.purchase_url,
		purchaseTitle: collapse(track.purchase_title),
		// What the uploader filed it under. '' is an answer ("no genre set"),
		// which the panel shows as "none".
		//
		// Adding this field is what `hasEveryField` below is for: an entry cached
		// before it existed has no `genre` key, and rather than being shown as an
		// unknown genre for the month its own lifetime had left, it counts as
		// stale and is asked about again — mostly for free, off the page's own
		// hydration.
		genre: collapse(track.genre),
		// The cover, for the rows that never showed one.
		//
		// A page lazy-loads its thumbnails as they scroll into view, so a list
		// read without scrolling through all of it hands over rows with no
		// artwork URL at all — and those rows drew a placeholder in the panel for
		// tracks whose artwork plainly exists. It was in the answer all along:
		// every track object SoundCloud sends here — the page's own hydration,
		// /tracks?ids=, the likes listing, /resolve — names its artwork, and this
		// keeps it. No extra request is made for it, and the page's own URL still
		// wins where there is one (see collectDownloadDetails): that rendition is
		// already in the browser's cache.
		artworkUrl: apiArtworkUrl(track)
	});

	// Details of the tracks among API objects (listing items are unwrapped by
	// the caller); objects that are not tracks, or lack a permalink, are dropped.
	const detailsOfTracks = (objects: ApiTrack[]) =>
		objects.flatMap((track) => (track.kind === 'track' && track.permalink_url ? [detailsOf(track)] : []));

	// ---- the cache: permalink path -> details (+ `at`), or a remembered
	// failure ({ unavailable: true, at }) so it is not retried right away.
	const detailsCache: Map<string, DetailsCacheEntry> = new Map();

	const ttlOf = (details: DetailsCacheEntry) => (details.unavailable ? FAILURE_TTL_MS : DETAILS_TTL_MS);

	// The fields detailsOf keeps. An entry that is missing one of them was
	// written by an older version of this extension, before that field was
	// kept.
	const DETAILS_FIELDS = [
		'id',
		'permalink',
		'downloadable',
		'hasDownloadsLeft',
		'purchaseUrl',
		'purchaseTitle',
		'genre',
		'artworkUrl'
	];

	// Whether an entry has every field this version writes.
	//
	// Age is not the only way a cache entry goes off. The details kept here
	// have grown — the genre was added to them after the cache had been filling
	// up for a month — and an entry from before that will never gain the field
	// by waiting: it is fresh, so nothing asks about the track again, so the
	// panel says the genre is unknown about a track SoundCloud would have named
	// the genre of in the same request it was going to make anyway. An entry
	// that predates a field is therefore stale whatever its age.
	//
	// What it costs is one round of looking up, once, on each page whose tracks
	// were cached by an older version — and most of that is free, because the
	// hydration harvest runs on every page load and refills from the page
	// itself. What it buys is that adding a field to detailsOf is enough:
	// nobody has to remember to invalidate anything.
	//
	// Remembered failures are exempt: they are not details and have none of
	// these fields, and their own day-long TTL is what expires them.
	const hasEveryField = (details: DetailsCacheEntry) =>
		details.unavailable === true || DETAILS_FIELDS.every((field) => field in details);

	const isFresh = (details: DetailsCacheEntry | undefined): details is DetailsCacheEntry =>
		!!details && Number.isFinite(details.at) && details.at > Date.now() - ttlOf(details) && hasEveryField(details);

	// Usable details (fresh, and not a remembered failure).
	const isKnown = (details: DetailsCacheEntry | undefined): details is KnownDetails =>
		isFresh(details) && !details.unavailable;

	// Whether the cache has an answer for a permalink, details or a remembered
	// failure — nothing more needs asking for it.
	const isSettled = (permalink: string) => isFresh(detailsCache.get(permalink));

	const loadDetailsCache = async () => {
		const stored = (await chrome.storage.local.get(DETAILS_KEY))[DETAILS_KEY];
		detailsCache.clear();

		for (const [permalink, details] of Object.entries(stored || {})) {
			if (isFresh(details)) detailsCache.set(permalink, details);
		}
	};

	// `entries` as an object, at most `limit` of them — the ones learned most
	// recently, since those are the pages being browsed now and the ones a
	// second visit is likeliest to want. (Every entry has been through isFresh,
	// so `at` is a number.)
	const newest = (entries: [string, DetailsCacheEntry][], limit: number) =>
		Object.fromEntries(
			entries.length <= limit ? entries : [...entries].sort((a, b) => b[1].at - a[1].at).slice(0, limit)
		);

	// Writes the cache back, merged with what is stored (another tab may have
	// added entries meanwhile), without expired entries and capped at CACHE_MAX.
	const saveDetailsCache = async () => {
		const stored = (await chrome.storage.local.get(DETAILS_KEY))[DETAILS_KEY];
		const merged: Map<string, DetailsCacheEntry> = new Map();

		for (const [permalink, details] of Object.entries(stored || {})) {
			if (isFresh(details)) merged.set(permalink, details);
		}

		for (const [permalink, details] of detailsCache) merged.set(permalink, details);
		const entries = [...merged];

		try {
			await chrome.storage.local.set({ [DETAILS_KEY]: newest(entries, CACHE_MAX) });
		} catch (_e) {
			// The area would not take it: the marks share it, and an entry can be
			// bigger than the cap was drawn for. Half as much rather than nothing —
			// a smaller cache still spares SoundCloud most of the asking, while a
			// write that gave up would send every track back to it next visit.
			await chrome.storage.local.set({
				[DETAILS_KEY]: newest(entries, Math.floor(CACHE_MAX / 2))
			});
		}
	};

	// The cache is written back a moment after it changes (one write per burst
	// of answers), and when a lookup ends.
	let saveTimer: ReturnType<typeof setTimeout> | undefined;

	const scheduleSave = () => {
		if (saveTimer) return;
		saveTimer = setTimeout(() => {
			saveTimer = undefined;
			saveDetailsCache().catch(() => {});
		}, SAVE_DELAY_MS);
	};

	const flushSave = () => {
		if (!saveTimer) return;
		clearTimeout(saveTimer);
		saveTimer = undefined;
		saveDetailsCache().catch(() => {});
	};

	// What follows anything newly in the cache: requests waiting on those
	// tracks are answered and the panel hears of the progress (noteProgress,
	// below), markers are repainted, and the cache is written back shortly.
	const cacheChanged = () => {
		noteProgress();
		requestMarkers();
		scheduleSave();
	};

	// Puts freshly learned details in the cache. Returns how many.
	const learn = (list: Omit<KnownDetails, 'at'>[]) => {
		let count = 0;

		for (const details of list) {
			detailsCache.set(details.permalink, { ...details, at: Date.now() });
			count++;
		}

		if (count > 0) cacheChanged();

		return count;
	};

	// Remembers that a track could not be looked up (for a day).
	const learnFailure = (permalink: string) => {
		detailsCache.set(permalink, { unavailable: true, at: Date.now() });
		cacheChanged();
	};

	// Whether details describe a track with no download link at all.
	// (`downloadable` is null when only the page's purchase link is known —
	// that never counts as "none".)
	const hasNoDownloadLink = (details: TrackDetails | null) =>
		!!details && details.downloadable === false && !details.purchaseUrl;

	// Every full track object the hydration holds goes into the cache. Free,
	// and run on every page load (see startMarkers).
	const harvestHydration = () => {
		const fresh: Omit<KnownDetails, 'at'>[] = [];

		const take = (track: ApiTrack) => {
			if (track.kind !== 'track' || !track.permalink_url) return;
			const details = detailsOf(track);

			if (isKnown(detailsCache.get(details.permalink))) return;
			fresh.push(details);
		};

		for (const entry of domHydration() || []) {
			const data = entry.data;

			if (!data) continue;
			take(apiTrackOf(data));
			apiPlaylistOf(data).tracks?.forEach(take);
		}

		return learn(fresh);
	};

	// ---- playlist pages

	// Details for the tracks of the playlist at `path`, learned as they come:
	// the full ones from the playlist data, then /tracks?ids= batches of its
	// stubs (skipping ids already cached) in playlist order, only until every
	// `wanted` (rendered) permalink is settled. Whatever is left over is
	// resolved individually by the caller.
	const playlistDetails = async (path: string, wanted: string[], clientId: string) => {
		const isCurrent = (playlist: ApiPlaylist | undefined): playlist is ApiPlaylist & { tracks: ApiTrack[] } => {
			const playlistPath = pathOf(playlist?.permalink_url);

			// A private playlist's URL carries a "/s-<token>" suffix.
			return (
				!!playlistPath &&
				Array.isArray(playlist!.tracks) &&
				(path === playlistPath || path.startsWith(`${playlistPath}/`))
			);
		};

		// Whichever hydration entry is this playlist (regular ones hydrate as
		// "playlist"; system playlists differ), else — stale hydration after
		// in-app navigation — one /resolve of its URL for the track ids.
		// (The hydration parsed by now — the client id came from it — so the
		// empty fallback is for safety; hence the coverage hint.)
		let playlist: ApiPlaylist | undefined = /* v8 ignore next */ (domHydration() || [])
			.map((entry) => apiPlaylistOf(entry.data))
			.find(isCurrent);

		if (!playlist) {
			playlist = apiPlaylistOf(
				await resolveOnce(`playlist:${path}`, () => apiGet('/resolve', { url: location.origin + path }, clientId))
			);
		}

		if (!isCurrent(playlist)) return;

		const knownIds = new Set([...detailsCache.values()].map((d) => d.id));
		const full = [];
		const stubIds = [];

		for (const track of playlist.tracks) {
			if (track.permalink_url) {
				full.push(detailsOf(track));
			} else if (track.id && !knownIds.has(track.id)) {
				stubIds.push(track.id);
			}
		}

		learn(full);

		const unsettled = () => wanted.some((p) => !isSettled(p));
		const batches = [];

		for (let i = 0; i < stubIds.length; i += API_BATCH) {
			batches.push(stubIds.slice(i, i + API_BATCH).join(','));
		}

		/** @param ids comma-separated. */
		const fetchBatch = async (ids: string) => {
			learn(detailsOfTracks(listOf(await apiGet('/tracks', { ids }, clientId)).map(apiTrackOf)));
		};

		// How wide to go: how many batches could hold a rendered row.
		//
		// The rows are the playlist's first rows and the stubs are in playlist
		// order, so `wanted` rows reach at most this far down the batches. A page
		// that has not been scrolled wants thirty rows, which is one batch, and
		// asks for exactly one — the other nine of a five-hundred-track playlist
		// are never made. A page that has been scrolled to the end wants all ten
		// and says so up front, rather than being made to discover it a doubling
		// at a time: the widening this replaces cost a scrolled page a whole
		// extra round trip, which is half a second to two seconds of the wait it
		// was meant to shorten.
		//
		// Only the width is decided here. The pool still walks every batch and
		// still stops the moment the rendered rows are settled, so a page whose
		// rows are not where this assumes loses nothing — it asks a little more
		// narrowly and the fallback resolves whatever is left.
		const reach = Math.max(1, Math.ceil(wanted.length / API_BATCH));
		await pool(batches, Math.min(reach, API_CONCURRENCY), fetchBatch, unsettled);
	};

	// ---- Likes pages

	// Id of the user whose likes the page lists — `owner` is 'you' or the
	// profile permalink: the signed-in user, else the profile in the hydration
	// when it is that one (in-app navigation can leave another profile there),
	// else one /resolve.
	const likesUserId = async (owner: string, clientId: string): Promise<number | null> => {
		const hydration = domHydration();

		if (owner === 'you') {
			return integerOf(objectOf(hydratable(hydration, 'meUser')).id) || null;
		}

		const user = objectOf(hydratable(hydration, 'user'));
		const userId = integerOf(user.id);

		if (user.permalink === owner && userId) return userId;

		const found = objectOf(
			await resolveOnce(`user:${owner}`, () => apiGet('/resolve', { url: `${location.origin}/${owner}` }, clientId))
		);

		const foundId = integerOf(found.id);

		return found.kind === 'user' && foundId ? foundId : null;
	};

	// Where a previous walk of a user's likes listing stopped: the `next_href`
	// it had reached and how many items into the listing that was.
	//
	// A Likes page is browsed by scrolling, and every scroll brings rows the
	// panel has not been asked about before — so the same listing is walked
	// again and again in one visit, each time from the top. Two thousand likes
	// read in ten scrolls is ten walks of one, two, three... ten pages: fifty-five
	// requests for the ten pages SoundCloud actually had to send. Carrying the
	// cursor makes it ten.
	//
	// Only sound when the rows now being asked about lie past what the earlier
	// walk covered, which is the scrolling case and is checked by the caller
	// (`from` below). Anything else — a filtered list, a like added at the top
	// since — starts from the top as before.
	// user id -> { next, seen }
	const likesCursor: Map<number, { next: string; seen: number }> = new Map();

	// Details from the likes listing, learned page by page. `depth` is how many
	// liked items (tracks and playlists) the page shows down to the deepest
	// wanted row and `from` the shallowest such row; the first request asks for
	// `depth` items (plus a little slack, since items the anonymous listing omits
	// shift the rest up) and later ones for the maximum, until every wanted
	// permalink is settled or the listing has been followed LIKES_MAX_PAGES deep.
	const likesDetails = async (owner: string, wanted: string[], depth: number, from: number, clientId: string) => {
		const userId = await likesUserId(owner, clientId);

		if (!userId) return;

		const unsettled = () => wanted.some((p) => !isSettled(p));

		// Pick up where the last walk left off when every wanted row is past it.
		const resume = likesCursor.get(userId);
		let seen = resume && from >= resume.seen ? resume.seen : 0;
		let url = seen ? resume!.next : `/users/${userId}/likes`;

		let params: ApiQuery = seen
			? { limit: LIKES_PAGE }
			: { limit: Math.min(LIKES_PAGE, depth + 5), offset: 0, linked_partitioning: 1 };

		for (let page = 0; page < LIKES_MAX_PAGES && unsettled(); page++) {
			let data: Json;

			try {
				data = await apiGet(url, params, clientId);
			} catch (e) {
				// A resumed cursor SoundCloud no longer honours is not a failed
				// lookup: drop it and let the caller's fallback resolve the rows.
				if (seen) likesCursor.delete(userId);
				throw e;
			}

			const listing = objectOf(data);
			const collection = listOf(listing.collection);
			// A like is a track's or a playlist's, and a playlist's has no `track`.
			learn(detailsOfTracks(collection.map((item) => apiTrackOf(objectOf(item).track))));
			seen += collection.length;
			const next = stringOf(listing.next_href);

			if (!next) break;
			url = next;
			params = { limit: LIKES_PAGE };
			likesCursor.set(userId, { next: url, seen });
		}
	};

	// ---- one track at a time (the fallback)

	// Resolves each permalink, API_CONCURRENCY at a time, learning each answer
	// (or failure) as it comes. Returns whether SoundCloud rejected the client
	// id (401), in which case the rest is left alone rather than hammered.
	const resolveDetails = async (permalinks: string[], clientId: string) => {
		let unauthorized = false;

		await pool(
			permalinks,
			API_CONCURRENCY,
			async (permalink) => {
				let details = null;

				try {
					const track = apiTrackOf(await apiGet('/resolve', { url: location.origin + permalink }, clientId));

					if (track.kind === 'track') {
						details = { ...detailsOf(track), permalink };
					}
				} catch (e) {
					// 401 means the client id is no longer accepted — that says nothing
					// about the track, so it is not remembered as a failure.
					if (e instanceof ApiError && e.status === 401) unauthorized = true;
					else learnFailure(permalink);

					return;
				}

				// Learned outside the try. Learning tells the panel, and telling the
				// panel throws once the extension has been reloaded under the page —
				// which is nothing wrong with the track, and must not be remembered
				// over an answer that has just arrived as if it were.
				if (details) learn([details]);
				else learnFailure(permalink);
			},
			() => !unauthorized
		);

		return unauthorized;
	};

	// ---- one lookup at a time
	//
	// The panel can be closed and reopened, and the user can navigate away and
	// back in-app, while a long lookup is still running; each reopen asks
	// again. Rather than starting a second lookup that competes with the first
	// for the same tracks, requests are queued: a request whose tracks are all
	// covered by a lookup already queued or running simply waits for those
	// tracks to be settled (it is answered as soon as they are, even while that
	// lookup goes on with the rest); any other request gets a lookup of its
	// own, run after the ones before it and asking only for what is still
	// unknown by then. One lookup runs at a time per tab, so SoundCloud never
	// sees two from it at once — a request for a different page waits its turn.

	const jobs: Job[] = []; // lookups queued and running, in order
	const waiters: Waiter[] = []; // requests waiting for their tracks to be settled

	const finishWaiter = (waiter: Waiter) => {
		const i = waiters.indexOf(waiter);

		// Callers only pass waiters still in the list (each is finished once);
		// the guard is for safety, hence the coverage hint.
		/* v8 ignore else */
		if (i >= 0) waiters.splice(i, 1);
		waiter.resolve({
			unauthorized: waiter.job.unauthorized,
			unseen: !!waiter.job.unseen,
			joined: waiter.joined
		});
	};

	// Progress for the panel, if it is still open: how many of the latest
	// request's tracks are settled. Nobody listening is fine.
	const reportProgress = (waiter: Waiter) => {
		// What has been learned since this waiter was last told, so the panel can
		// fill its rows in as the answers arrive rather than all at once at the
		// end. Each track is handed over once; a remembered failure settles a row
		// without giving it anything to show, so it is counted and not sent.
		const learned: KnownDetails[] = [];

		for (const permalink of waiter.wanted) {
			if (waiter.told.has(permalink)) continue;
			const details = detailsCache.get(permalink);

			if (!isFresh(details)) continue;
			waiter.told.add(permalink);

			if (!details.unavailable) learned.push(details);
		}

		chrome.runtime
			.sendMessage({
				type: 'DOWNLOAD_DETAILS_PROGRESS',
				done: waiter.wanted.filter(isSettled).length,
				total: waiter.wanted.length,
				resumed: waiter.joined,
				details: learned
			} satisfies DetailsProgress)
			.catch(() => {});
	};

	// After the cache changed (an answer arrived, or another tab learned
	// something): answers requests whose tracks are all settled and reports
	// how far the latest one is.
	const noteProgress = () => {
		// A copy, since finishing a waiter takes it out of the list.
		for (const waiter of waiters.slice()) {
			if (waiter.wanted.every(isSettled)) finishWaiter(waiter);
		}

		const latest = waiters[waiters.length - 1];

		if (latest) reportProgress(latest);
	};

	// One lookup: the bulk source for the page's kind first, then whatever is
	// left one track at a time — for what is still unsettled when its turn
	// comes (earlier lookups, or another tab, may have covered some meanwhile).
	const runJob = async (job: Job) => {
		const unsettled = () => job.wanted.filter((p) => !isSettled(p));
		let pending = unsettled();
		// What the bulk source was given to do, and whether it did any of it.
		const before = pending.length;
		let bulk = false;

		if (pending.length && job.page.type === 'playlist') {
			bulk = true;

			try {
				await playlistDetails(job.page.key, pending, job.clientId);
			} catch (_e) {
				// Fall through: the rows are resolved individually below.
			}

			pending = unsettled();
		}

		if (pending.length && job.page.type === 'likes') {
			// The listing is worth it when it costs fewer requests than resolving
			// each pending track: one request covers up to LIKES_PAGE items.
			const pendingSet = new Set(pending);
			const indexes = job.items.filter((item) => pendingSet.has(item.permalink)).map((item) => item.itemIndex || 0);
			const depth = 1 + Math.max(0, ...indexes);
			// The shallowest row still wanted, which says whether a walk of this
			// listing that stopped earlier can simply be carried on (see
			// likesCursor).
			const from = Math.min(...indexes, depth);

			if (pending.length > Math.ceil(depth / LIKES_PAGE)) {
				bulk = true;

				try {
					await likesDetails(likesOwner(job.page), pending, depth, from, job.clientId);
				} catch (_e) {
					// Fall through.
				}

				pending = unsettled();
			}
		}

		// ---- nothing to be seen
		//
		// These requests are anonymous — the extension asks SoundCloud the way a
		// signed-out visitor would, which is what keeps it from needing the user's
		// account. A page whose tracks are not public is therefore one the API
		// will not describe however it is asked, and the ordinary case of that is
		// somebody looking at their own private playlist.
		//
		// The bulk source comes back empty for such a page, and resolving each row
		// in turn then fails the same way one request at a time: ninety tracks is
		// ninety round trips, a minute of "still checking", and an answer of no.
		// Measured on a private 90-track playlist, that was the whole of the wait.
		//
		// So when the bulk source settled nothing at all, a few rows are resolved
		// first. If even one comes back, the rest follow as they always did — the
		// page is visible and the bulk source simply did not cover it (a filtered
		// list, rows past a listing's reach). If none do, the rest are left
		// alone: they are the same request, and it has been asked.
		if (pending.length && bulk && pending.length === before && pending.length > PROBE) {
			const probe = pending.slice(0, PROBE);
			job.unauthorized = await resolveDetails(probe, job.clientId);

			// A rejected key is not an invisible page, and it is not a reason to ask
			// again either: the rest are left alone whichever of the two it was.
			if (job.unauthorized) return;

			if (!probe.some((p) => isKnown(detailsCache.get(p)))) {
				job.unseen = true;

				return;
			}

			pending = unsettled();
		}

		if (pending.length) {
			job.unauthorized = await resolveDetails(pending, job.clientId);
		}
	};

	// Has the tracks in `wanted` (permalinks the cache cannot answer for)
	// looked up: by a lookup already queued that covers them all, else by a new
	// one queued behind the others. `page` is the page they are on and `items`
	// its tracks ({ permalink, itemIndex }), which the bulk sources need.
	// Resolves to { unauthorized, unseen, joined } once every one of them is
	// settled or the lookup they were left to is over.
	const lookUp = (page: Job['page'], items: Job['items'], wanted: string[], clientId: string): Promise<LookupOutcome> =>
		new Promise((resolve) => {
			let job = jobs.find((j) => wanted.every((p) => j.wantedSet.has(p)));
			const joined = !!job;

			if (!job) {
				job = {
					page,
					items,
					wanted,
					wantedSet: new Set(wanted),
					clientId,
					unauthorized: false,
					unseen: false
				};
				const previous = jobs.length ? jobs[jobs.length - 1].done! : Promise.resolve();
				job.done = previous
					.then(() => runJob(job!))
					.catch(() => {
						// Whatever went wrong, what was learned stays; the rest is
						// reported as not checked.
					})
					.finally(() => {
						jobs.splice(jobs.indexOf(job!), 1);

						for (const waiter of waiters.slice()) {
							if (waiter.job === job) finishWaiter(waiter);
						}

						flushSave();
					});
				jobs.push(job);
			}

			waiters.push({ wanted, job, joined, resolve, told: new Set() });
			noteProgress();
		});

	// ---- putting it together

	// Extraction plus a `details` field per track: cached/looked-up details,
	// else — for Likes rows whose purchase link the page rendered — partial
	// details with `downloadable: null`, else null. Also `lookupFailed` (how
	// many tracks could not be looked up), `resumed` (whether a lookup already
	// under way covered this request, so it waited for that one) and `error`
	// ('' | 'no-client-id' | 'unauthorized').
	const collectDownloadDetails = async (): Promise<DetailsAnswer> => {
		const result: DetailsAnswer = {
			...extract(),
			lookupFailed: 0,
			resumed: false,
			// Whether the lookup gave up because nothing it asked about came
			// back — a page the anonymous API cannot see (see runJob).
			unseen: false,
			error: ''
		};

		if (result.tracks.length === 0) return result;

		const clientId = pageClientId();

		if (!clientId) {
			result.error = 'no-client-id';

			return result;
		}

		harvestHydration();

		// Rendered tracks the cache cannot answer for (recent failures included).
		const wanted = [
			...new Set(
				result.tracks.map((track) => track.permalink).filter((permalink) => permalink && !isSettled(permalink))
			)
		];

		if (wanted.length > 0) {
			const { unauthorized, unseen, joined } = await lookUp(
				{ type: result.pageType, key: result.pageKey },
				result.tracks.map(({ permalink, itemIndex }) => ({ permalink, itemIndex })),
				wanted,
				clientId
			);

			result.resumed = joined;
			result.unseen = unseen;

			if (unauthorized) result.error = 'unauthorized';
			// Wanted tracks now remembered as failures failed during this lookup
			// (earlier failures were not wanted in the first place).
			result.lookupFailed = wanted.filter((permalink) => {
				const details = detailsCache.get(permalink);

				return isFresh(details) && details.unavailable;
			}).length;
		}

		for (const track of result.tracks) {
			const cached = detailsCache.get(track.permalink);

			if (isKnown(cached)) {
				track.details = cached;
			} else if (track.domPurchaseUrl) {
				// What the page itself shows: the purchase link, nothing about
				// SoundCloud's own download.
				track.details = {
					id: null,
					permalink: track.permalink,
					downloadable: null,
					hasDownloadsLeft: null,
					purchaseUrl: track.domPurchaseUrl,
					purchaseTitle: track.domPurchaseTitle || '',
					// These are the details, which is what SoundCloud was asked for,
					// and it was not asked — so this says nothing about the genre, as
					// the nulls above say nothing about the download. (`''` would be an
					// answer: "the uploader set none".) What the row itself rendered is
					// not lost by being absent here: it is on the track, in `genre`,
					// which is where the page's own answers live and where the panel
					// reads it from first.
					genre: null,
					// Read off the page, which is where the artwork was read from too —
					// so there is nothing here the row does not already have.
					artworkUrl: '',
					partial: true
				};
			} else {
				track.details = null;
			}

			// A row whose thumbnail never lazy-loaded has no cover to show, but the
			// answer just given names one. The page's own URL wins where there is
			// one — it is the rendition already in the browser's cache.
			if (!track.artworkUrl && track.details) {
				track.artworkUrl = track.details.artworkUrl || '';
			}
		}

		return result;
	};

	// ---- Markers ---------------------------------------------------------------
	//
	// Tracks the user has downloaded get a translucent overlay on their cover
	// art with a "Downloaded" pill in its bottom-right corner; tracks whose
	// details are known (looked up or harvested) and offer no download link at
	// all get a dark "nodl" pill there (styles in content.css). One pill per
	// track: downloaded wins, being the end of the story. Being in the session
	// queue is not a pill at all — the queue button below says that itself, in
	// the corner where a click can do something about it.
	//
	// Wherever the track's cover art is: the rows and tiles of a list, a track
	// page's own hero, and the player bar. The same two pills in all of them,
	// from the same answer for the same track, so what a page says about a
	// track does not depend on which of its pictures is being looked at.
	//
	// Downloaded marks live in the extension's own storage (see
	// downloaded-store.js) and the queue in its session storage (see
	// src/lib/queue.svelte.js), both out of the page's reach; the overlay
	// element is the only thing added to the page, and it is inert
	// (pointer-events: none) — the queue button below it is the one addition
	// that takes a click. A MutationObserver re-applies markers as SoundCloud
	// renders rows lazily or navigates in-app, and lookups do so as answers
	// arrive.

	const MARK_CLASS = 'tbx-mark';

	// Put on the artwork a marker sits in, so it is the overlay's containing

	// block. See content.css.

	const MARK_HOST_CLASS = `${MARK_CLASS}-host`;

	// Where each layout keeps its cover art, and how the track it belongs to is
	// named: `link` is a selector for the link that names it, `key` a function
	// for the places where no link does. The artwork element is
	// position: relative in every one of them, so the overlay can be an
	// absolutely positioned child (content.css says so itself for the marked
	// one, rather than trusting SoundCloud to keep it that way). Cover art of
	// 32 px or so is too small for "Downloaded", so those get the compact
	// variant.
	// `fields` reads the rest of the track out of the same element, for the
	// queue button below — everything a queued entry keeps: the title, artist
	// and artwork, and the three things only the page can say (the genre it
	// renders beside the row, and whether the row's own like and repost buttons
	// are on). Those three are read here rather than left to the panel because
	// the page is the only place they exist: a track sitting in the queue has
	// no row to go back and read, and one added to the Manager's list from the
	// queue would otherwise arrive knowing none of them.
	// It is called on a click, not on a paint: a marked-up playlist repaints
	// hundreds of rows at a time, and only one of them is ever queued.
	const MARKER_TARGETS: MarkerTarget[] = [
		{
			rows: TRACK_ROW_SELECTOR,
			art: '.trackItem__image .sc-artwork',
			link: ROW_TITLE,
			variant: 'small',
			fields: (row) => ({
				title: textOf(row, ROW_TITLE),
				artist: textOf(row, ROW_ARTIST),
				artworkUrl: artworkUrlOf(row, ROW_ART),
				genre: genreOf(row),
				...engagementOf(row)
			})
		},
		{
			rows: LIKED_SOUND_SELECTOR,
			art: '.sound__coverArt .sc-artwork',
			link: SOUND_TITLE,
			variant: 'large',
			fields: (row) => ({
				title: textOf(row, SOUND_TITLE),
				artist: soundArtistOf(row),
				artworkUrl: artworkUrlOf(row, SOUND_ART),
				genre: genreOf(row),
				...engagementOf(row)
			})
		},
		{
			rows: LIKED_TILE_SELECTOR,
			art: '.playableTile__image .sc-artwork',
			link: '.playableTile__mainHeading, .playableTile__artworkLink',
			variant: 'large',
			fields: (tile) => ({
				title: tileTitleOf(tile),
				artist: textOf(tile, TILE_ARTIST),
				artworkUrl: artworkUrlOf(tile, TILE_ART),
				genre: genreOf(tile),
				...engagementOf(tile)
			})
		},
		{
			// A track page's own hero artwork. Nothing in the hero names the track
			// — its title is an h1, not a link — so the page itself does, through
			// detectPage; that is also what keeps a playlist's hero clear, since
			// the same .fullHero renders there and a playlist is not a track to
			// mark. The hero nests a second .sc-artwork inside the first for the
			// image itself; querySelector takes the outer one, which is the box
			// the pill belongs in.
			rows: HERO_ROWS,
			art: '.listenArtworkWrapper__artwork .sc-artwork',
			key: () => {
				const page = detectPage();

				return page.type === 'track' ? page.key : '';
			},
			variant: 'large',
			fields: heroFieldsOf
		},
		{
			// The player bar, so what is playing says what it is however far the
			// page has scrolled. Its 30 px artwork takes the compact variant. The
			// title is a link for a real track and a bare <span> for an ad, which
			// names nothing: permalinkPathOf gives '' for that and no mark follows.
			rows: '.playbackSoundBadge',
			art: '.playbackSoundBadge__avatar .sc-artwork',
			link: 'a.playbackSoundBadge__titleLink',
			variant: 'small',
			fields: (badge) => ({
				title: textOf(badge, '.playbackSoundBadge__titleLink'),
				// The bar names the uploader in a second link, which is not there
				// while a track is still loading.
				artist: textOf(badge, '.playbackSoundBadge__lightLink'),
				artworkUrl: artworkUrlOf(badge, '.playbackSoundBadge__avatar .image__full')
				// The bar carries no tag and none of the row buttons, so a track
				// queued from it knows no more than it says: genre, like and repost
				// are left unsaid rather than answered from somewhere else.
			})
		}
	];

	// The track a marker target's element belongs to: the link that names it,
	// or the target's own answer where the page holds no such link.
	const markKeyOf = (target: MarkerTarget, row: ParentNode) =>
		target.key ? target.key() : permalinkPathOf(row.querySelector(target.link!));

	let downloaded: DownloadedMarks = {}; // permalink path -> ISO time, mirrored from storage

	// The session queue's permalinks, mirrored from chrome.storage.session. The
	// key is the one src/lib/queue.svelte.js writes; a content script is an
	// untrusted context, so it can only reach that area at all because the
	// popup opens it up (queue.svelte.js again) — which is why the button below
	// only appears once the queue has answered.
	const QUEUE_KEY = 'queue';
	let queued: Set<string> = new Set();

	// Whether that door is open: set the first time the area answers a read, or
	// sends a change (see the storage listener in startMarkers). Until then the
	// page shows no queued pills and offers no queue button,
	// because a click on one could not be written anywhere.
	let queueReachable = false;

	// Whether what the area holds under QUEUE_KEY is the queue. The popup's
	// queue store and this script are alone in writing there, and write a list
	// of entries; anything else there counts as no queue at all.
	const isQueue = (stored: unknown): stored is QueueEntry[] => Array.isArray(stored);

	const queuedFrom = (items: QueueEntry[]): Set<string> =>
		new Set(items.flatMap((item) => (item?.permalink ? [item.permalink] : [])));

	// The queue as it is stored, entries and all. Throws where the area is shut.
	const loadQueueItems = async (): Promise<QueueEntry[]> => {
		const stored = await chrome.storage.session.get(QUEUE_KEY);

		return isQueue(stored[QUEUE_KEY]) ? stored[QUEUE_KEY] : [];
	};

	const loadQueue = async (): Promise<Set<string>> => {
		try {
			const items = await loadQueueItems();
			queueReachable = true;

			return queuedFrom(items);
		} catch (_e) {
			// No session storage to read here (the popup has not opened it up, or
			// this browser keeps it shut): no queued pills, everything else as it
			// was.
			return new Set();
		}
	};

	// Which marker a track should carry: 'downloaded', 'nodl', or ''. Being in
	// the queue is not one of them: the queue button in the opposite corner says
	// that itself (see "The queue button" below), which puts the state and the
	// click that changes it on the same control instead of splitting them across
	// two corners.
	const markKindOf = (permalink: string) => {
		if (!permalink) return '';

		if (downloaded[permalink]) return 'downloaded';
		const details = detailsCache.get(permalink);

		if (isKnown(details) && hasNoDownloadLink(details)) return 'nodl';

		return '';
	};

	const makeMarker = (kind: 'downloaded' | 'nodl', variant: 'small' | 'large') => {
		const overlay = document.createElement('div');
		overlay.className = `${MARK_CLASS} ${MARK_CLASS}--${kind} ${MARK_CLASS}--${variant}`;
		overlay.dataset.kind = kind;
		const pill = document.createElement('span');
		pill.className = `${MARK_CLASS}__pill`;

		if (kind === 'nodl') pill.textContent = 'nodl';
		// A 32 px thumbnail has no room for the word: "Downloaded" becomes the
		// check it means.
		else pill.textContent = variant === 'small' ? '✓' : 'downloaded';
		overlay.append(pill);

		return overlay;
	};

	// The mark on one artwork: added, swapped, or taken off again when the row
	// has been recycled for another track. Nothing is touched when it already
	// says the right thing, which is most of every repaint.
	const paintMark = (art: ParentNode, kind: 'downloaded' | 'nodl' | '', variant: 'small' | 'large') => {
		const existing = art.querySelector<HTMLElement>(`:scope > .${MARK_CLASS}`);

		if (existing && existing.dataset.kind === kind) return;

		if (existing) existing.remove();

		if (kind) art.append(makeMarker(kind, variant));
	};

	// ---- The queue button ------------------------------------------------------
	//
	// Hovering a track's cover art brings up a button in its top corner that
	// puts the track in the session queue, or takes it back out — the same
	// toggle the Manager panel puts on every row, on the page itself, so a
	// batch can be picked out while listening rather than afterwards. It is the
	// one thing the content script writes; everything else it does to the page
	// is a picture of something the popup owns.
	//
	// It is also what says a track *is* in the queue: a queued track's button
	// reads "queued" and stays up without being hovered, and turns into the
	// "unqueue" it would become under the pointer. One control, carrying the
	// state and the click that changes it — rather than a pill in one corner
	// saying what a track is and a button in the other disagreeing about which
	// corner to read. Everything the page knows and cannot be clicked stays a
	// pill (see "Markers"); the play button SoundCloud puts in the middle of the
	// same artwork is left alone, corner to centre.
	//
	// Not rendered at all until the queue has answered a read — see
	// `queueReachable`. A button that could not write anywhere would be worse
	// than no button.

	const QUEUE_BUTTON_CLASS = 'tbx-queue';

	// What the button says, by variant and by whether the track is queued
	// already: `text` at rest, `hover` in its place under the pointer. The swap
	// is the stylesheet's (content.css), so it costs no listener here and no
	// repaint — and the resting word stays the button's own text, which is what
	// a reader of the DOM gets. 32 px thumbnails and the player bar's 30 px one
	// have room for a sign and nothing else: there a queued track keeps the "+"
	// its pill used to carry, and hovering turns it into the "−" that would take
	// it out. "unqueue" is the word the panel's own batch action uses for the
	// same thing.
	const QUEUE_BUTTON_STATES = {
		'large:no': { text: '+ queue', hover: '', label: 'add to the session queue' },
		'large:yes': { text: 'queued', hover: 'unqueue', label: 'take out of the session queue' },
		'small:no': { text: '+', hover: '', label: 'add to the session queue' },
		'small:yes': { text: '+', hover: '−', label: 'take out of the session queue' }
	};

	// The details the queue should carry for a track: what has been looked up
	// for it, where that is known — which is how the Downloader finds its gate
	// later without a page to read — and nothing where it is not.
	const knownDetails = (permalink: string): KnownDetails | null => {
		const details = detailsCache.get(permalink);

		return isKnown(details) ? details : null;
	};

	// A track as the queue keeps it: the row's own words and picture, plus
	// whatever is known about it. A row whose thumbnail never lazy-loaded has no
	// picture to give, and takes the cover out of those details rather than
	// going into the queue with none (see detailsOf).
	/** @param fields {} where no layout's reader could find the row. */
	const queueEntryOf = (permalink: string, fields: Partial<RowFields>): QueueEntry => {
		const details = knownDetails(permalink);

		return {
			permalink,
			// The three the page is the only source of, as nulls first: a layout
			// whose reader says nothing about them — the player bar names a track,
			// not a row — leaves "nobody knows" rather than leaving the field out,
			// which is the same shape the popup's own queue writes.
			genre: null,
			liked: null,
			reposted: null,
			...fields,
			artworkUrl: fields.artworkUrl || (details && details.artworkUrl) || '',
			details
		};
	};

	// Writes the whole queue back with the track in it, or with it gone. It is
	// read again first rather than taken from the mirror above: another tab may
	// have queued something since this page last repainted, and this write
	// replaces the lot.
	//
	// The track is read off the row at this point rather than at paint time: a
	// marked-up playlist repaints hundreds of rows at once and only ever one of
	// them is clicked.
	const toggleQueued = async (permalink: string, target: MarkerTarget, row: ParentNode) => {
		try {
			const items = await loadQueueItems();
			const next = items.filter((item) => item?.permalink !== permalink);

			if (next.length === items.length) {
				next.push(queueEntryOf(permalink, target.fields(row)));
			}

			await chrome.storage.session.set({ [QUEUE_KEY]: next });
		} catch (_e) {
			// The door shut under us — the extension was reloaded beneath the page,
			// or session storage went away. The page keeps showing what it last
			// read; the popup still has the queue.
		}
	};

	// Puts a track in the queue if it is not in it already — which is what
	// `queue.add` does in the popup, and what a replaced download button needs:
	// a click on one is "I am getting this", not a toggle. Its caller is the
	// only one, and never has a track without a permalink: a control that names
	// none is left as SoundCloud drew it and has no button of ours to click.
	const enqueue = async (permalink: string, fields: Partial<RowFields>) => {
		try {
			const items = await loadQueueItems();

			if (items.some((item) => item?.permalink === permalink)) return;
			await chrome.storage.session.set({
				[QUEUE_KEY]: [...items, queueEntryOf(permalink, fields)]
			});
		} catch (_e) {
			// The door is shut (the popup has not opened it this session) or it
			// shut under us. The link still opens and the file still downloads;
			// the queue is the part that misses out.
		}
	};

	const makeQueueButton = (art: ParentNode, target: MarkerTarget, row: ParentNode) => {
		const button = document.createElement('button');
		button.type = 'button';
		button.className = `${QUEUE_BUTTON_CLASS} ${QUEUE_BUTTON_CLASS}--${target.variant}`;
		// The resting word goes in an element of its own, because the hovered one
		// is the stylesheet's `::after`: keeping them apart is what leaves the
		// button's text the one word it is actually showing.
		const text = document.createElement('span');
		text.className = `${QUEUE_BUTTON_CLASS}__text`;
		button.append(text);
		// The artwork is a play control, and in two of these layouts it sits
		// inside a link to the track: queueing must set neither off. The capture
		// listener further down is not among them — it watches SoundCloud's own
		// download controls, which this is not.
		button.addEventListener('click', (event) => {
			event.preventDefault();
			event.stopPropagation();
			void toggleQueued(button.dataset.permalink!, target, row);
		});
		art.append(button);

		return button;
	};

	// The button on one artwork: put there, kept in step with the queue, or
	// taken off again once the row names no track (an ad in the player bar) or
	// the queue turns out to be out of reach.
	/** @param permalink '' where the row names no track. */
	const paintQueueButton = (art: ParentNode, permalink: string, target: MarkerTarget, row: ParentNode) => {
		const existing = art.querySelector<HTMLButtonElement>(`:scope > .${QUEUE_BUTTON_CLASS}`);

		if (!permalink || !queueReachable) {
			if (existing) existing.remove();

			return;
		}

		const button = existing || makeQueueButton(art, target, row);
		// Which track the click will queue. Read at click time, because the row
		// it was painted on can be recycled for another track before then.
		button.dataset.permalink = permalink;
		const state: keyof typeof QUEUE_BUTTON_STATES = `${target.variant}:${queued.has(permalink) ? 'yes' : 'no'}`;

		if (button.dataset.state === state) return;
		button.dataset.state = state;
		const { text, hover, label } = QUEUE_BUTTON_STATES[state];
		button.querySelector(`:scope > .${QUEUE_BUTTON_CLASS}__text`)!.textContent = text;

		// No hover word where hovering changes nothing but the shade: the
		// stylesheet swaps only what carries one.
		if (hover) button.dataset.hover = hover;
		else delete button.dataset.hover;
		button.title = label;
		button.setAttribute('aria-label', label);
	};

	// Adds missing markers and buttons, swaps changed ones, and removes stale
	// ones (a row can be recycled for a different track when the list changes).
	// Idempotent.
	const applyMarkers = () => {
		// First, because it can replace every row this pass is about to paint:
		// the Likes page's grid becomes a list, and the mutation that does it
		// brings another pass round for the rows. Painting the doomed tiles
		// anyway costs one pass over a dozen of them and cannot leave the page
		// unmarked should the toggle ever stop answering.
		applyLikesView();

		for (const target of MARKER_TARGETS) {
			for (const row of document.querySelectorAll(target.rows)) {
				const art = row.querySelector(target.art);

				if (!art) continue;
				const permalink = markKeyOf(target, row);
				const kind = markKindOf(permalink);
				paintMark(art, kind, target.variant);
				paintQueueButton(art, permalink, target, row);
				// The artwork is made the containing block for whichever of the two
				// it carries. The class goes on the element itself rather than being
				// matched with `:has()`; see content.css for what that cost.
				art.classList.toggle(MARK_HOST_CLASS, Boolean(kind || (permalink && queueReachable)));
			}
		}

		// SoundCloud's own download buttons ride along on the same pass: they
		// come and go with the rows, and the More menu renders when it is opened.
		// A menu therefore shows SoundCloud's own button for as long as this pass
		// is coalesced for, which is a tenth of a second of a menu animating open.
		applyDownloadButtons();
	};

	// Repaints markers shortly, coalescing bursts of changes into one pass.
	let markerTimer: ReturnType<typeof setTimeout> | undefined;

	const requestMarkers = () => {
		if (markerTimer) return;
		markerTimer = setTimeout(() => {
			markerTimer = undefined;
			applyMarkers();
		}, 150);
	};

	const onMutations = (mutations: MutationRecord[]) => {
		// Only element insertions can bring new rows; text updates (the player's
		// clock, counters) are ignored to keep this cheap.
		const relevant = mutations.some((mutation) =>
			[...mutation.addedNodes].some((node) => node.nodeType === Node.ELEMENT_NODE)
		);

		if (relevant) requestMarkers();
	};

	const startMarkers = async () => {
		[downloaded, queued] = await Promise.all([TbxDownloaded.load(), loadQueue(), loadDetailsCache(), loadSettings()]);
		// Whatever full tracks this page was served with are free to keep (they
		// are written back by learn's follow-up).
		harvestHydration();
		applyMarkers();
		new MutationObserver(onMutations).observe(document.body, {
			childList: true,
			subtree: true
		});
		chrome.storage.onChanged.addListener((changes, area) => {
			// The queue is in its own area, and is the whole of what that area has
			// to say here.
			if (area === 'session') {
				if (changes[QUEUE_KEY]) {
					// Chrome tells a page about this area only once it may read it, so
					// a change is the door opening — the popup, opened after this page
					// loaded — even where the first read found it shut.
					queueReachable = true;
					const items = changes[QUEUE_KEY].newValue;
					queued = queuedFrom(isQueue(items) ? items : []);
					applyMarkers();
				}

				return;
			}

			if (area !== 'local') return;

			if (changes[TbxDownloaded.KEY]) {
				const marks = changes[TbxDownloaded.KEY].newValue;
				downloaded = TbxDownloaded.isMarks(marks) ? marks : {};
			}

			if (changes[DETAILS_KEY]) {
				// Another tab looked tracks up: take what it learned — it may settle
				// tracks a request here is waiting for.
				let learned = false;

				for (const [permalink, details] of Object.entries(changes[DETAILS_KEY].newValue || {})) {
					const known = detailsCache.get(permalink);

					if (isFresh(details) && (!known || known.at < details.at)) {
						detailsCache.set(permalink, details);
						learned = true;
					}
				}

				if (learned) noteProgress();
			}

			if (changes[SETTINGS_KEY]) {
				// SAFETY: chrome.storage keeps what JSON can hold, and a value it
				// hands back is one it kept.
				settings = settingsOf(changes[SETTINGS_KEY].newValue as Json | undefined);
				// A settings change is a fresh ask: the Likes page is put into its
				// List view again if it is showing the grid, whichever way the
				// setting was left before this.
				listViewAsked = false;
			}

			if (changes[TbxDownloaded.KEY] || changes[DETAILS_KEY] || changes[SETTINGS_KEY]) {
				applyMarkers();
			}
		});
	};

	// ---- Noticing SoundCloud's own download buttons ---------------------------
	//
	// Downloads made through SoundCloud's UI are marked too: "Download file" in
	// a track's More menu, and the purchase link ("Buy", "Free Download", …) in
	// Likes rows and on track pages. A capture-phase listener sees the click
	// before the page does and never stops or alters it. The More menu renders
	// detached from its row (appended to <body>), so the row whose More button
	// was clicked last is remembered to attribute the download.

	// Permalink of the track an element belongs to: its row on list pages, or
	// the page's own track when inside a track page's hero or action bar (see
	// detectPage for how that track is named).
	const trackOfElement = (el: Element) => {
		const row = el.closest('.trackItem, .sound, .audibleTile');

		const fromRow = row
			? permalinkPathOf(
					row.querySelector(
						'.trackItem__trackTitle, .soundTitle__title, .playableTile__mainHeading, .playableTile__artworkLink'
					)
				)
			: '';

		if (fromRow) return fromRow;
		const page = detectPage();

		if (page.type === 'track' && page.key && el.closest(`.listenEngagement, .fullHero, .l-listen-hero, ${V2_HEADER}`)) {
			return page.key;
		}

		return '';
	};

	let lastMoreContext = '';

	// And the More button itself, so the row it sits in can be read for the
	// rest of the track — the title, artist and artwork a queued entry keeps
	// (see fieldsAround). The menu is detached from that row; this is the only
	// thread back to it.
	let lastMoreElement: Element | null = null;

	// Storage can be gone if the extension was reloaded under an open page
	// ("extension context invalidated"); that is not worth a console error.
	/** @param permalink '' where nothing names the track, which marks nothing. */
	const mark = (permalink: string) => {
		if (permalink) TbxDownloaded.set(permalink, true).catch(() => {});
	};

	document.addEventListener(
		'click',
		(event) => {
			const el = event.target instanceof Element ? event.target : null;

			if (!el) return;

			const more = el.closest('.sc-button-more');

			if (more) {
				lastMoreContext = trackOfElement(more);
				lastMoreElement = more;

				return;
			}

			const download = el.closest('.sc-button-download');

			if (download) {
				mark(trackOfElement(download) || lastMoreContext);

				return;
			}

			const purchase = el.closest(`.soundActions__purchaseLink, .sc-buylink, ${V2_HEADER} a[target="_blank"]`);

			if (purchase) mark(trackOfElement(purchase));
		},
		true
	);

	// ---- The settings the page obeys -----------------------------------------
	//
	// Two of them so far, and both are about what this script does to a page
	// somewhere the popup is not: whether SoundCloud's own download buttons are
	// replaced (the section below), and whether the Likes page is put into its
	// List view (the section after that). They are written by the popup's
	// settings sheet into chrome.storage.local, under one key, as one object —
	// see src/lib/settings.svelte.js for why they cannot live where a panel's
	// own options do.

	const SETTINGS_KEY = 'settings';

	// What each is before anyone has touched it, written down here as well as in
	// src/lib/settings.svelte.js: a classic script in the page has no imports,
	// so the two cannot share a line of it. test/consistency.test.ts holds them
	// together.
	const SETTINGS_DEFAULTS: PageSettings = {
		replaceDownloadButtons: true,
		likesListView: false
	};

	let settings = { ...SETTINGS_DEFAULTS };

	// Stored settings are not this version's to trust — an older install, a
	// later one, or a hand-edited storage — so anything but a boolean, under
	// any name, is the default.
	const settingsOf = (stored: Json | undefined): PageSettings => {
		const held = objectOf(stored);

		const setting = (name: keyof PageSettings) => {
			const value = held[name];

			return value === true || value === false ? value : SETTINGS_DEFAULTS[name];
		};

		return { replaceDownloadButtons: setting('replaceDownloadButtons'), likesListView: setting('likesListView') };
	};

	const loadSettings = async () => {
		try {
			const stored: Partial<Record<string, Json>> = await chrome.storage.local.get(SETTINGS_KEY);
			settings = settingsOf(stored[SETTINGS_KEY]);
		} catch (_e) {
			// No storage to read: the defaults stand, which is the same answer the
			// popup's own settings sheet would show.
		}
	};

	// ---- Replacing SoundCloud's own download buttons --------------------------
	//
	// SoundCloud's own way to a file is two controls: "Download file", an item
	// in each track's More menu, and the purchase link ("Buy", "Free Download",
	// …) on Likes rows and track pages. Turned on (it is, by default — see
	// src/lib/settings.svelte.js), this puts the Manager panel's own button
	// in their place, doing what that button does for the same track: the track
	// goes into the session queue, and then the link opens or the file is
	// downloaded.
	//
	// Why the queue: opening a download link says nothing that is known yet —
	// the file has not been saved and the gate behind the link has not even
	// been walked — so the panel queues the track and the queue sheet marks a
	// whole batch of them as downloaded once the files really are. A click here
	// means the same thing it means there, so it does the same thing.
	//
	// Ours says one word wherever it stands — "Download", with the icon that
	// makes it a download at a glance — and leaves the rest to its tooltip: the
	// domain the file would come from, which is the thing that differs between
	// one of these buttons and the next and the thing worth knowing before
	// clicking. It is drawn the height of the controls standing beside it, so it
	// sits in the row it joins rather than on top of it.
	//
	// The two are not quite the same underneath, and one of them cannot be:
	//
	//   - a purchase link is a link, so ours is one too — a real <a> to the
	//     link's real destination (SoundCloud wraps it in the gate.sc
	//     redirector; the panel unwraps it, and so does this). The browser
	//     opens it, which means a ⌘-click still puts it behind the page, and
	//     nothing here has to ask for a tab.
	//   - "Download file" has no URL at all: it is a button whose click
	//     SoundCloud answers with the file. The panel's button for such a
	//     track opens the *track page*, where that menu item is, and the click
	//     detection above then marks it — which on the track page itself would
	//     be a button that reopens the page it is on and downloads nothing. So
	//     ours presses SoundCloud's own button, hidden beneath it. The end
	//     state is the one the panel arrives at (queued, then marked by the
	//     click detection above), one step shorter.
	//
	// Nothing is removed from the page: SoundCloud's control is hidden with a
	// class of ours and pressed, or shown again untouched the moment the
	// setting goes off — or the moment a control turns out to name no track,
	// which is the one thing that must never leave an empty space where a
	// download button was.

	// SoundCloud's controls, by the kind of link the panel would make of them.
	// The legacy `.sc-buylink` is matched as well, exactly as the click
	// detection above matches it.
	const NATIVE = {
		soundcloud: '.sc-button-download',
		external: '.soundActions__purchaseLink, .sc-buylink',
		// The same purchase link in the signed-in layout (see V2_HEADER above),
		// which names nothing: an <a> around an icon button, with no class worth
		// the name on either. It is picked out by where it is and where it goes —
		// the one link in the track header that leaves the site — because that is
		// what stays true when the next build renames every class in the frame.
		v2: `${V2_HEADER} a[target="_blank"]`
	};

	const OURS_CLASS = 'tbx-dl';
	// Put on SoundCloud's control while ours stands in for it. See content.css.
	const NATIVE_HIDDEN_CLASS = 'tbx-dl-replaced';

	// The one word every one of these buttons says. What it does is the same
	// everywhere it stands, so it reads the same everywhere it stands, and the
	// one thing that differs — where the file comes from — is in the tooltip
	// rather than in a label that used to be the gate's own wording.
	const OURS_TEXT = 'Download';

	// Drawn rather than written, so a download is a download at a glance in a
	// row of SoundCloud's own controls. Lucide's `download` — the set the
	// popup's own buttons use — as paths, so nothing has to be loaded for it.
	const SVG_NS = 'http://www.w3.org/2000/svg';
	const ICON_PATHS = ['M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4', 'M7 10l5 5 5-5', 'M12 15V3'];

	const makeIcon = () => {
		const svg = document.createElementNS(SVG_NS, 'svg');
		svg.setAttribute('class', `${OURS_CLASS}__icon`);
		svg.setAttribute('viewBox', '0 0 24 24');
		svg.setAttribute('fill', 'none');
		svg.setAttribute('stroke', 'currentColor');
		svg.setAttribute('stroke-width', '2');
		svg.setAttribute('stroke-linecap', 'round');
		svg.setAttribute('stroke-linejoin', 'round');
		// A picture of the word beside it: nothing for a screen reader to read.
		svg.setAttribute('aria-hidden', 'true');

		for (const d of ICON_PATHS) {
			const path = document.createElementNS(SVG_NS, 'path');
			path.setAttribute('d', d);
			svg.append(path);
		}

		return svg;
	};

	// The domain a click would fetch the file from, which is what the tooltip
	// says. "www." is dropped: it is noise, and the panel's own grouping drops
	// it too.
	const hostOf = (href: string) => {
		try {
			return new URL(href).hostname.replace(/^www\./, '');
		} catch (_e) {
			// A gate.sc wrapper can carry anything at all in its `url`, including
			// something that is no URL: then the destination itself is the most
			// that can be said about where this goes.
			return '';
		}
	};

	// Every button SoundCloud draws carries some form of this class — sc-button,
	// sc-button-small, sc-button-like, sc-button-more — so it is how the
	// controls standing beside ours are found without naming each layout's own.
	//
	// In the signed-in frame there is no such class on anything: its row is MUI
	// icon buttons, whose only unhashed class is `MuiIconButton-root`. That one
	// is MUI's own API rather than a build artefact, so it is nameable.
	const NEIGHBOUR = '[class*="sc-button"], .MuiIconButton-root';

	// How tall the row's own controls stand, as a CSS length: ours is drawn to
	// match, which is the whole of what "the same height as the buttons next to
	// it" means. Measured rather than written down, because the rows are
	// SoundCloud's and it sizes their buttons itself — small in a Likes row's
	// actions, taller in a track page's engagement bar, a full menu row inside
	// More.
	//
	// The *neighbours* are measured, not the control being replaced. Two reasons,
	// and the first is the ask: what ours has to line up with is the buttons
	// beside it, and SoundCloud does not always size a purchase link like the
	// ones it sits among. The second is that the control being replaced is
	// hidden the moment ours goes in, so it has no height to give on any pass
	// after the first — while its neighbours are still there to ask, which is
	// what lets an answer be found later (a More menu is measured while it is
	// still opening, and 0 is not an answer).
	//
	// Siblings first, then the siblings of whatever wraps it: a menu puts each
	// item in a list item of its own, so the buttons to match are a level
	// further out. Three levels up and it is no longer the same row.
	const neighbourHeight = (native: Element) => {
		let el = native;

		for (let up = 0; up < 3 && el.parentElement; up += 1, el = el.parentElement) {
			for (const sibling of el.parentElement.children) {
				if (sibling === el) continue;
				const control = sibling.matches(NEIGHBOUR) ? sibling : sibling.querySelector(NEIGHBOUR);
				const height = control instanceof HTMLElement ? control.offsetHeight : 0;

				if (height) return height;
			}
		}

		return 0;
	};

	// Remembered per kind of control, because reading an element's height makes
	// the browser lay the page out and a Likes page has hundreds of rows to put
	// buttons in: the first of a kind measures, the rest are drawn to its answer
	// without touching the page. A row standing on its own, with no other
	// control in it, falls back to the one being replaced — which is only ever
	// readable on the pass that hides it.
	/** Kind and class -> the height, as a CSS length. */
	const rowHeights: Map<string, string> = new Map();

	/** @returns a CSS length, or '' where nothing had a height to give yet. */
	const rowHeight = (native: HTMLElement, kind: string) => {
		// The kind is part of the key because one of these controls has no class
		// to be told apart by: the signed-in layout's purchase link is a bare
		// <a>, and '' must not be the same row as anything else's ''.
		const key = `${kind}|${native.className}`;
		const known = rowHeights.get(key);

		if (known) return known;
		const height = neighbourHeight(native) || native.offsetHeight;

		if (!height) return '';
		const style = `${height}px`;
		rowHeights.set(key, style);

		return style;
	};

	// The track an element sits inside, as the queue would keep it. Every
	// layout's own reader, through the same table the markers use, so a track
	// queued from a download button carries the same title as the same track
	// queued from its cover art or from the panel.
	/** @returns `{}` where no layout's reader finds a track around it. */
	const fieldsAround = (el: Element): Partial<RowFields> => {
		// The More menu renders detached from its row, so the row whose More
		// button was clicked last stands in for it — the same stand-in the
		// permalink takes above.
		for (const from of [el, lastMoreElement]) {
			if (!from) continue;

			for (const target of MARKER_TARGETS) {
				const row = from.closest(target.rows);

				if (row) return target.fields(row);
			}
		}

		// Inside none of them: a track page's action bar sits below the hero
		// rather than in it. The hero describes the page's own track — and only a
		// track page has one, a playlist's hero being the very same element, so
		// the page has to say which it is (as the hero's own marker does).
		if (detectPage().type !== 'track') return {};
		const hero = document.querySelector(HERO_ROWS);

		if (hero) return heroFieldsOf(hero);
		// The signed-in layout's header, which is the same hero by another name
		// (see V2_HEADER above) and holds the action row this button is in.
		const header = document.querySelector(V2_HEADER);

		return header ? v2FieldsOf(header) : {};
	};

	// Which track our button in one of SoundCloud's places is for and where its
	// file would come from, or null where there is nothing to put there — no
	// track to name it with, or a purchase link that points nowhere. Null leaves
	// SoundCloud's own control exactly as it was. What the button *says* is not
	// here: it is the same word every time.
	/** @param kind a key of NATIVE. */
	const replacementFor = (native: Element, kind: string) => {
		// The More menu renders detached from its row, so the row whose More
		// button was clicked last is what names its "Download file" — the same
		// attribution the click detection above makes.
		const permalink = kind === 'soundcloud' ? trackOfElement(native) || lastMoreContext : trackOfElement(native);

		if (!permalink) return null;
		const details = knownDetails(permalink);

		if (kind === 'soundcloud') {
			// Spent: SoundCloud still offers the download, but its limit for the
			// period is used up, so the button is there and gives nothing. The
			// panel says so on the same track; so does this.
			const spent = !!details && details.downloadable && !details.hasDownloadsLeft;

			// The domain, and the one thing that changes what a click gets: the
			// panel says as much on the same track.
			return {
				permalink,
				href: '',
				from: spent ? 'soundcloud.com (no downloads left)' : 'soundcloud.com'
			};
		}

		// Both remaining kinds are a link to somewhere else: the classic
		// purchase link and the signed-in layout's cart, which is the same link
		// drawn as an icon. Where it really goes is the whole of what differs.
		const href = unwrapGate(native.getAttribute('href'));

		if (!href) return null;

		return { permalink, href, from: hostOf(href) || href };
	};

	// Our button, put beside the control it stands in for. A link where there
	// is somewhere to go, a button where the click itself is the whole of it.
	/**
	 * @param kind a key of NATIVE.
	 * @param href '' for SoundCloud's own "Download file", which has none.
	 */
	const makeOurButton = (native: HTMLElement, kind: string, href: string) => {
		const el = document.createElement(href ? 'a' : 'button');
		el.className = `${OURS_CLASS} ${OURS_CLASS}--${kind}`;

		if (!href) el.type = 'button';
		el.append(makeIcon());
		const text = document.createElement('span');
		text.className = `${OURS_CLASS}__text`;
		text.textContent = OURS_TEXT;
		el.append(text);
		el.addEventListener('click', () => {
			// SoundCloud's own button first, so the file is asked for inside the
			// click that asked for it: a browser hands a gesture on for a few
			// seconds and there is nothing here worth spending them on. A link
			// needs none of this — the browser is already following it.
			if (kind === 'soundcloud') native.click();
			// Read at click time, because a row can be recycled for another track
			// between the paint that made this button and the click on it.
			void enqueue(el.dataset.permalink!, fieldsAround(native));
		});
		native.after(el);

		return el;
	};

	// One pass over SoundCloud's own controls: ours put in their place, kept in
	// step with what has since been looked up, or taken away again. Idempotent,
	// which matters because what the tooltip says — the domain, and whether
	// SoundCloud's download limit is spent — is learned after the page is drawn.
	const applyDownloadButtons = () => {
		for (const [kind, selector] of Object.entries(NATIVE)) {
			// Buttons and links, every one of them (see NATIVE).
			for (const native of document.querySelectorAll<HTMLElement>(selector)) {
				const plan = settings.replaceDownloadButtons ? replacementFor(native, kind) : null;
				const neighbour = native.nextElementSibling;
				const ours = neighbour instanceof HTMLElement && neighbour.classList.contains(OURS_CLASS) ? neighbour : null;

				if (!plan) {
					// Off, or nothing to offer: SoundCloud's page, as SoundCloud drew it.
					native.classList.remove(NATIVE_HIDDEN_CLASS);

					if (ours) ours.remove();
					continue;
				}

				const button = ours || makeOurButton(native, kind, plan.href);
				native.classList.add(NATIVE_HIDDEN_CLASS);

				// Asked again on every pass until there is an answer, and never after:
				// a menu that was still opening the first time round has a height by
				// the next one, and a button already drawn to a height costs nothing
				// to leave alone.
				if (!button.style.height) button.style.height = rowHeight(native, kind);
				button.dataset.permalink = plan.permalink;

				if (plan.href) button.setAttribute('href', plan.href);
				// Where from, rather than what: the word is the same on every one of
				// these, so this is the part worth a tooltip — and the part a screen
				// reader would otherwise never reach.
				button.title = plan.from;
				button.setAttribute('aria-label', `download from ${plan.from}`);
			}
		}
	};

	// ---- The Likes page's view -----------------------------------------------
	//
	// /you/likes has two views, chosen by SoundCloud's own "View" toggle above
	// the list: Badges, a grid of tiles, and List, the rows every other Likes
	// page draws. SoundCloud remembers which was last picked and opens the page
	// in it, and the one it ships with is Badges.
	//
	// Which is unfortunate, because a tile says less than a row does — about
	// half of what this extension can read off a Likes page is only in the
	// rows. A tile renders no genre tag, so the genre of every liked track has
	// to be looked up instead of read; no like or repost button, so the panel
	// cannot say whether a track is liked from that page at all; and no
	// purchase link, so there is nothing there to replace with a download
	// button. The README has twice told people to switch that page to List for
	// exactly those reasons. Turned on (it is not, by default), this does it
	// for them.
	//
	// It presses SoundCloud's own toggle rather than restyling anything: the
	// grid is SoundCloud's to draw, and the page it draws afterwards is the one
	// a person would have got by pressing the same button. That also means
	// SoundCloud remembers the choice, so the page stays a list until something
	// asks it not to be.
	//
	// Once per arrival at the page, and no more: a click on Badges afterwards
	// is a person saying they want the grid this time, and this must not take
	// it back from under them. The guard lifts when the page is left (the
	// toggle is no longer rendered) and when the setting itself is changed.

	// The toggle, and the half of it that asks for rows. Only the signed-in
	// user's own Likes page renders either — a public /<user>/likes is a list
	// already and has no toggle — so the selector is the whole of the gate on
	// which page this runs on.
	const VIEW_TOGGLE = '.collectionSection .listDisplayToggle';
	const LIST_TOGGLE = '.listDisplayToggle__listToggle';

	// What the Badges view draws. Read rather than the toggle's own pressed
	// state, because this is the thing that actually has to change, and one
	// liked item of it is enough to know the grid is up.
	const BADGE_GRID = '.collectionSection .badgeList';

	let listViewAsked = false;

	// Asks for the List view, if this is that page, it is showing the grid, and
	// nothing has asked yet. Idempotent by the guard rather than by its effect:
	// pressing the toggle a second time would put the grid back.
	const applyLikesView = () => {
		const toggle = document.querySelector<HTMLElement>(`${VIEW_TOGGLE} ${LIST_TOGGLE}`);

		if (!toggle) {
			// Somewhere else now (or not there yet): the next arrival is a fresh
			// one, and gets its own press.
			listViewAsked = false;

			return;
		}

		if (!settings.likesListView || listViewAsked) return;

		if (!document.querySelector(BADGE_GRID)) return;
		// Set before the click, not after: SoundCloud may re-render inside it,
		// and a pass that ran from within this one must not press it again.
		listViewAsked = true;
		toggle.click();
	};

	// ---- Artist profiles -----------------------------------------------------
	//
	// Who is behind a track and where else to find them, for the artists tool:
	// the uploader's public profile, and the links they put on it. Two requests
	// per artist, to the same public API the details use and on the same terms
	// — anonymous, GET, the page's own client id: /resolve of the profile URL
	// for the user (id, name, avatar, place, follower count, bio), then
	// /users/<id>/web-profiles for the links. Four at a time, as the details
	// are; a 401 stops the lot, since it says nothing about any one artist.
	//
	// Nothing here is cached beyond the document. The panel keeps what it is
	// told on the entry it asked for, and asks only about the ones it has no
	// answer for; the /resolve half is shared with the Likes lookup through
	// `resolveOnce`, so a profile asked about twice in one page is one request.

	const BIO_MAX = 1000; // characters of a bio the panel keeps
	const LINKS_MAX = 20; // links per profile
	const ARTIST_PATH = /^\/[^/?#\s]+$/;

	const isArtistPath = (path: Json): path is string => isString(path) && ARTIST_PATH.test(path);

	// The links a profile lists, as the panel keeps them: the network SoundCloud
	// files each under, the uploader's own title for it, and the address.
	// Anything that is not an http(s) address is dropped.
	const profileLinksOf = (list: Json): ProfileLink[] =>
		listOf(list)
			.map(objectOf)
			.filter((one) => /^https?:\/\//i.test(stringOf(one.url).trim()))
			.slice(0, LINKS_MAX)
			.map((one) => ({
				network: stringOf(one.network || one.service).toLowerCase(),
				title: collapse(stringOf(one.title)),
				url: stringOf(one.url).trim()
			}));

	// The subset of an API user object the panel keeps, filed under the path it
	// was asked for.
	// `collapse` takes a string or nothing; the API is free to send anything.
	const wordsOf = (value: Json | undefined) => collapse(stringOf(value));

	/** @param links what /users/<id>/web-profiles answered, or []. */
	const profileOf = (permalink: string, user: ApiUser, links: Json): ArtistProfile => ({
		id: integerOrNull(user.id),
		permalink,
		username: wordsOf(user.username),
		fullName: wordsOf(user.full_name),
		city: wordsOf(user.city),
		country: wordsOf(user.country_code),
		followers: integerOrNull(user.followers_count),
		trackCount: integerOrNull(user.track_count),
		verified: user.verified === true,
		avatarUrl: stringOf(user.avatar_url),
		bio: stringOf(user.description).trim().slice(0, BIO_MAX),
		links: profileLinksOf(links)
	});

	// One artist: the user behind the path, else null for a path that is not a
	// public profile. The links are asked for once the user is known; a profile
	// whose links cannot be fetched is still a profile, with none.
	const fetchProfile = async (permalink: string, clientId: string): Promise<ArtistProfile | null> => {
		const user = objectOf(
			await resolveOnce(`user:${permalink.slice(1)}`, () =>
				apiGet('/resolve', { url: location.origin + permalink }, clientId)
			)
		);

		// Asked after by whatever id the answer gives; profileOf keeps only a whole number.
		const id = stringOf(user.id) || integerOf(user.id);

		if (user.kind !== 'user' || !id) return null;
		let links: Json = [];

		try {
			links = await apiGet(`/users/${id}/web-profiles`, {}, clientId);
		} catch (e) {
			if (e instanceof ApiError && e.status === 401) throw e;
		}

		return profileOf(permalink, user, links);
	};

	// The panel's question: `profiles` for the paths that are public profiles,
	// `failed` for the ones that are not (or could not be asked about), and
	// `error` ('' | 'no-client-id' | 'unauthorized'). Progress goes out as it
	// comes, an artist at a time, so the rows fill in rather than wait.
	/** @param permalinks the panel's list, checked path by path. */
	const collectArtistProfiles = async (permalinks: Json | undefined): Promise<ProfilesAnswer> => {
		const result: ProfilesAnswer = { ok: true, profiles: [], failed: [], error: '' };
		const wanted = [...new Set(listOf(permalinks).filter(isArtistPath))];

		if (wanted.length === 0) return result;

		const clientId = pageClientId();

		if (!clientId) {
			result.error = 'no-client-id';

			return result;
		}

		let unauthorized = false;
		let done = 0;

		const report = (profiles: ArtistProfile[], failed: string[]) => {
			chrome.runtime
				.sendMessage({
					type: 'ARTIST_PROFILES_PROGRESS',
					done,
					total: wanted.length,
					profiles,
					failed
				} satisfies ProfilesProgress)
				.catch(() => {});
		};

		await pool(
			wanted,
			API_CONCURRENCY,
			async (permalink) => {
				let profile = null;

				try {
					profile = await fetchProfile(permalink, clientId);
				} catch (e) {
					// 401 means the client id is no longer accepted, which says nothing
					// about this artist and stops the rest; anything else is this one.
					if (e instanceof ApiError && e.status === 401) {
						unauthorized = true;

						return;
					}
				}

				done += 1;

				if (profile) result.profiles.push(profile);
				else result.failed.push(permalink);
				report(profile ? [profile] : [], profile ? [] : [permalink]);
			},
			() => !unauthorized
		);

		if (unauthorized) result.error = 'unauthorized';

		return result;
	};

	// ---- Messages from the panels --------------------------------------------

	// Marks and the details cache are loaded before any lookup runs, so a panel
	// opened right after a page load does not ask for tracks already known.
	const ready = startMarkers().catch(() => {});

	// Only the tab's own document answers the panels. The manifest says
	// `all_frames` so that the signed-in track page — which is an iframe (see
	// V2_HEADER above) — gets its purchase link replaced and a click on
	// SoundCloud's own noticed. (Its artwork is not marked: no marker target
	// reads that layout yet.) A frame that also answered would be a second
	// reply to a question that takes one: `chrome.tabs.sendMessage` without a
	// frame id asks every frame in the tab and keeps whichever answers first,
	// so a panel would sometimes read the page and sometimes read the frame.
	// The frame does the work that is about the page it is in; what a panel
	// asks for comes from the top.
	if (window.top === window) {
		chrome.runtime.onMessage.addListener((message: PageRequest, _sender, sendResponse) => {
			if (!message) return undefined;

			if (message.type === 'EXTRACT_TRACKS') {
				sendResponse(extract());

				return undefined; // sent synchronously
			}

			if (message.type === 'GET_DOWNLOAD_DETAILS') {
				ready
					.then(collectDownloadDetails)
					.then(sendResponse, (e) => sendResponse({ ok: false, error: String((e && e.message) || e) }));

				return true; // response is sent asynchronously
			}

			if (message.type === 'GET_ARTIST_PROFILES') {
				collectArtistProfiles(message.permalinks).then(sendResponse, (e) =>
					sendResponse({ ok: false, error: String((e && e.message) || e) })
				);

				return true;
			}

			return undefined;
		});
	}
})();
