// Builders for the SoundCloud markup the content script reads — the same
// structure and class names the README documents (verified against
// soundcloud.com in September 2026), reduced to the elements that matter.
//
// Every builder returns an HTML string; `render` puts it on the page.
import type { Json } from './json.ts';

const esc = (s: string | number | null | undefined) =>
	String(s ?? '')
		.replace(/&/g, '&amp;')
		.replace(/</g, '&lt;')
		.replace(/>/g, '&gt;')
		.replace(/"/g, '&quot;');

// SoundCloud sets an inline background-image on span.image__full once the
// thumbnail has lazy-loaded; before that the span carries only sizing.
const imageFull = (url: string | null | undefined) =>
	url
		? `<span class="image__full" style="background-image: url(&quot;${esc(url)}&quot;); width: 100%; height: 100%; opacity: 1;"></span>`
		: '<span class="image__full" style="width: 100%; height: 100%; opacity: 0;"></span>';

const gateSc = (url: string) => `https://gate.sc?url=${encodeURIComponent(url)}&token=abc123`;

// The badge SoundCloud nests *inside* the uploader link on a verified
// account's playlist and track heroes. Its SVG <title> is text, so anything
// reading the link's textContent would take the name as "Uploader Verified".
const verifiedBadge = () => '<span class="verifiedBadge"><svg><title>Verified</title></svg></span>';

// The genre tag SoundCloud renders beside a title — one `.sc-tag` holding the
// uploader's genre. Sound-list rows carry it in the title block's tag
// container; a track page's hero renders the same tag without one. `null`
// renders none, which is what a playlist row and a badge tile do.
const genreTag = (genre: string | null) =>
	genre === null
		? ''
		: `<a class="sc-tag soundTitle__tag sc-tag-small"><span class="sc-truncate sc-tagContent">${esc(genre)}</span></a>`;

const tagContainer = (genre: string | null) =>
	genre === null
		? ''
		: `<div class="soundTitle__additionalContainer"><div class="soundTitle__tagContainer">${genreTag(genre)}</div></div>`;

/** A like or repost button: off, on by both tells, or on by one of them alone. */
export type Pressed = boolean | 'class' | 'aria';

interface Engagement {
	liked?: Pressed;
	reposted?: Pressed;
	actions?: boolean;
}

/** A purchase link as the uploader set it, before gate.sc wraps it. */
export interface Purchase {
	url: string;
	label: string;
}

// What each builder below takes; every option has a default there.
export interface PlaylistRowOptions extends Engagement {
	artist?: string;
	title?: string;
	href?: string | null;
	artwork?: string | null;
	system?: boolean;
	itemClass?: string;
}

export interface PlaylistPageOptions {
	title?: string;
	curator?: string;
	curatorHref?: string;
	curatorBadge?: boolean;
	setHref?: string;
	/** The rows' markup (playlistRow). */
	rows?: string[];
	engagement?: boolean;
	summary?: boolean;
	trackList?: boolean;
}

export interface SystemPlaylistPageOptions {
	title?: string;
	curator?: string;
	/** The rows' markup (playlistRow with `system: true`). */
	rows?: string[];
	hero?: boolean;
}

export interface LikedSoundOptions extends Engagement {
	artist?: string;
	displayArtist?: string;
	artistHref?: string;
	title?: string;
	href?: string | null;
	artwork?: string | null;
	playlist?: boolean;
	playlistClass?: boolean;
	purchase?: Purchase | null;
	usernameText?: boolean;
	genre?: string | null;
}

export interface LikedTileOptions {
	artist?: string;
	title?: string;
	href?: string;
	artworkHref?: string | null;
	artwork?: string | null;
	icon?: boolean;
	heading?: boolean;
}

export interface TrackPageOptions extends Engagement {
	uploader?: string;
	uploaderName?: string;
	title?: string;
	purchase?: Purchase | null;
	hero?: boolean;
	engagement?: boolean;
	artwork?: string;
	genre?: string | null;
	uploaderBadge?: boolean;
}

export interface V2TrackPageOptions {
	uploader?: string;
	uploaderName?: string;
	title?: string;
	purchase?: Purchase | null;
	artwork?: string | null;
	genre?: string | null;
	titleAttr?: boolean;
	uploaderLink?: boolean;
	headers?: number;
}

export interface PlayerBarOptions {
	title?: string;
	href?: string | null;
	artwork?: string | null;
	artist?: string | null;
}

export interface ApiTrackOptions {
	id?: number;
	permalink?: string;
	downloadable?: boolean;
	hasDownloadsLeft?: boolean;
	purchaseUrl?: string | null;
	purchaseTitle?: string | null;
	genre?: string | null;
	artworkUrl?: string | null;
	avatarUrl?: string | null;
	kind?: string;
}

// A row's like and repost buttons, which SoundCloud renders on every row of a
// playlist or Likes page, signed in or not. An "on" one carries both tells the
// content script reads — `sc-button-selected` and aria-pressed — and each can
// be given on its own, to stand for a redesign that drops the other.
//
// `actions: false` leaves them out altogether, which is what the badge tiles
// do and what a row that never rendered them looks like: the reader answers
// null there rather than "not liked".
const engagementButtons = ({ liked = false, reposted = false, actions = true }: Engagement = {}) => {
	if (!actions) return '';

	const button = (kind: string, state: Pressed) => {
		const on = state === 'class' || state === true;
		const pressed = state === 'aria' || state === true;

		return `<button class="sc-button-${kind}${on ? ' sc-button-selected' : ''}" type="button" aria-pressed="${pressed}">${kind}</button>`;
	};

	return button('like', liked) + button('repost', reposted);
};

export const ART = (hash: string, size = 't120x120') => `https://i1.sndcdn.com/artworks-${hash}-${size}.jpg`;

// A track row on a playlist page (li.trackList__item .trackItem). `href` is
// the title link exactly as SoundCloud renders it, "?in=" query included.
export function playlistRow({
	artist = 'Artist',
	title = 'Title',
	href = '/artist/title?in=curator/sets/my-playlist',
	artwork = ART('row'),
	system = false,
	// Row wrapper class; SoundCloud may rename it (see the .trackItem fallback).
	itemClass = system ? 'systemPlaylistTrackList__item' : 'trackList__item',
	// The like and repost buttons: false (rendered, off), true (both tells),
	// 'class' or 'aria' (one of them). `actions: false` renders neither button.
	liked = false,
	reposted = false,
	actions = true
}: PlaylistRowOptions = {}) {
	return `
    <li class="${itemClass}">
      <div class="trackItem">
        <div class="trackItem__image">
          <div class="sc-artwork sc-artwork-placeholder-1">${imageFull(artwork)}</div>
        </div>
        <div class="trackItem__content">
          <a class="trackItem__username" href="/${esc(artist.toLowerCase())}">${esc(artist)}</a>
          ${href === null ? `<span class="trackItem__trackTitle">${esc(title)}</span>` : `<a class="trackItem__trackTitle" href="${esc(href)}">${esc(title)}</a>`}
        </div>
        <div class="trackItem__actions"><button class="sc-button-more" type="button">More</button>${engagementButtons({ liked, reposted, actions })}</div>
      </div>
    </li>`;
}

// A regular playlist page: hero (title, curator, tracks summary) plus the
// rendered track list. `curatorBadge` nests a "Verified" badge in the curator
// link, whose SVG <title> would leak into textContent.
export function playlistPage({
	title = 'My Playlist',
	curator = 'Curator',
	curatorHref = '/curator',
	curatorBadge = true,
	setHref = '/curator/sets/my-playlist',
	rows = [],
	engagement = true,
	summary = true,
	trackList = true
}: PlaylistPageOptions = {}) {
	const badge = curatorBadge ? verifiedBadge() : '';

	return `
    <div class="l-listen-hero">
      <div class="fullHero">
        <div class="listenArtworkWrapper">
          <div class="listenArtworkWrapper__artwork">
            <div class="image sc-artwork sc-artwork-40x">${imageFull(ART('sethero', 't500x500'))}</div>
          </div>
        </div>
        <div class="fullHero__title">
          <div class="soundTitle">
            <h1 class="soundTitle__title"><a href="${esc(setHref)}"><span>${esc(title)}</span></a></h1>
            <h2 class="soundTitle__username"><a href="${esc(curatorHref)}">${esc(curator)}${badge}</a></h2>
          </div>
        </div>
        ${summary ? `<div class="fullHero__tracksSummary">${rows.length} tracks</div>` : ''}
      </div>
    </div>
    <div class="l-listen-wrapper">
      ${engagement ? '<div class="listenEngagement"><button class="sc-button-more" type="button">More</button></div>' : ''}
      ${trackList ? `<div class="listenDetails"><div class="trackList"><ul class="trackList__list">${rows.join('')}</ul></div></div>` : ''}
    </div>
    <div class="l-sidebar">
      <div class="sidebarModule">
        <div class="soundTitle__title">Unrelated sidebar card</div>
      </div>
    </div>`;
}

// A system playlist page (/discover/sets/<id>), whose hero and list use
// their own class names.
export function systemPlaylistPage({
	title = 'Daily Drops',
	curator = 'SoundCloud',
	rows = [],
	hero = true
}: SystemPlaylistPageOptions = {}) {
	return `
    ${
			hero
				? `<div class="systemPlaylistHero fullHero">
        <div class="fullHero__titleText">
          <div class="fullHero__titleTextLineSmall"><a href="/discover">${esc(curator)}</a></div>
          <h1 class="fullHero__titleTextTitle">${esc(title)}</h1>
        </div>
        <div class="fullHero__tracksSummary">${rows.length} tracks</div>
      </div>`
				: ''
		}
    <div class="systemPlaylistDetails">
      <ul class="systemPlaylistTrackList">${rows.join('')}</ul>
    </div>`;
}

// A liked item in the "sound list" layout (public /<user>/likes, and
// /you/likes in List view). A playlist row carries the "playlist" class and
// embeds a preview of its tracks under ul.trackList__item.
export function likedSound({
	artist = 'Uploader',
	displayArtist = artist,
	artistHref = '/uploader',
	title = 'Liked Track',
	href = '/uploader/liked-track',
	artwork = ART('sound', 't500x500'),
	playlist = false,
	playlistClass = playlist,
	purchase = null,
	usernameText = true,
	liked = false,
	reposted = false,
	actions = true,
	// The genre tag beside the title; `null` renders none.
	genre = null
}: LikedSoundOptions = {}) {
	const purchaseLink = purchase
		? `<a class="soundActions__purchaseLink sc-buylink" href="${esc(gateSc(purchase.url))}" aria-label="${esc(purchase.label)}">${esc(purchase.label)}</a>`
		: '';

	const preview = playlist
		? `<ul class="trackList__item">
        <li class="compactTrackList__item">
          <div class="trackItem">
            <a class="trackItem__username" href="/nested">Nested Artist</a>
            <a class="trackItem__trackTitle" href="/nested/track">Nested Track</a>
          </div>
        </li>
      </ul>`
		: '';

	return `
    <li class="soundList__item">
      <div>
        <div class="sound streamContext${playlistClass ? ' playlist' : ''}">
          <div class="sound__artwork">
            <div class="sound__coverArt"><div class="sc-artwork">${imageFull(artwork)}</div></div>
          </div>
          <div class="sound__content">
            <div class="soundTitle">
              <a class="soundTitle__username" href="${esc(artistHref)}">${usernameText ? `<span class="soundTitle__usernameText">${esc(displayArtist)}</span>` : esc(displayArtist)}</a>
              ${href === null ? `<span class="soundTitle__title">${esc(title)}</span>` : `<a class="soundTitle__title" href="${esc(href)}"><span>${esc(title)}</span></a>`}
              ${tagContainer(genre)}
            </div>
            <div class="soundActions">
              <button class="sc-button-more" type="button">More</button>
              ${engagementButtons({ liked, reposted, actions })}
              ${purchaseLink}
            </div>
            ${preview}
          </div>
        </div>
      </div>
    </li>`;
}

// A liked item in the badge grid layout (/you/likes, Badges view). Track and
// playlist tiles share their classes; only the hrefs tell them apart.
export function likedTile({
	artist = 'Uploader',
	title = 'Liked Tile',
	href = '/uploader/liked-tile',
	artworkHref = href,
	artwork = ART('tile', 't500x500'),
	icon = true,
	heading = true
}: LikedTileOptions = {}) {
	return `
    <li class="badgeList__item">
      <div class="audibleTile playableTile">
        <div class="playableTile__artwork">
          ${artworkHref === null ? '' : `<a class="playableTile__artworkLink" href="${esc(artworkHref)}">`}
            <div class="playableTile__image"><div class="sc-artwork">${imageFull(artwork)}</div></div>
          ${artworkHref === null ? '' : '</a>'}
        </div>
        <div class="playableTile__description">
          ${
						heading
							? `<a class="playableTile__mainHeading" href="${esc(href)}">${esc(title)}${icon ? '<span class="playableTile__icon">Track</span>' : ''}</a>`
							: ''
					}
          <a class="playableTile__usernameHeading" href="/${esc(artist.toLowerCase())}">${esc(artist)}</a>
        </div>
      </div>
    </li>`;
}

export const tilePlaceholder = () => '<li class="badgeList__item"><div class="audibleTilePlaceholder"></div></li>';

// A public Likes page: heading, tab bar with the active Likes tab, rows.
export function publicLikesPage({
	user = 'dj-someone',
	heading = 'Likes by DJ Someone',
	items = []
}: { user?: string; heading?: string; items?: string[] } = {}) {
	return `
    <div class="userNetworkTop"><h1 class="userNetworkTop__title">${esc(heading)}</h1></div>
    <ul class="userNetworkTabs g-tabs">
      <li class="networkTabs__likes g-tabs-item"><a class="g-tabs-link active" href="/${esc(user)}/likes">Likes</a></li>
      <li class="g-tabs-item"><a class="g-tabs-link" href="/${esc(user)}/followers">Followers</a></li>
    </ul>
    <ul class="soundList lazyLoadingList__list">${items.join('')}</ul>`;
}

// The signed-in user's own likes (/you/likes): the Library layout with the
// Badges/List view toggle; optionally the Library tab bar as well. The toggle
// halves carry their own classes, because one of them is a button the content
// script presses (the "open your likes as a list" setting); `toggle: false`
// drops the whole toggle, for the page that renders one no more.
export function ownLikesPage({
	items = [],
	list = false,
	tabs = false,
	toggle = true
}: { items?: string[]; list?: boolean; tabs?: boolean; toggle?: boolean } = {}) {
	return `
    ${
			tabs
				? '<ul class="g-tabs"><li class="g-tabs-item"><a class="g-tabs-link active" href="/you/likes">Likes</a></li><li class="g-tabs-item"><a class="g-tabs-link" href="/you/library">Overview</a></li></ul>'
				: ''
		}
    <div class="collectionSection">
      <div class="collectionSection__top">
        <h2>Likes</h2>
        ${
					toggle
						? `<div class="listDisplayToggle">
                <button type="button" class="listDisplayToggle__badgeToggle">Badges</button>
                <button type="button" class="listDisplayToggle__listToggle">List</button>
              </div>`
						: ''
				}
      </div>
      <ul class="${list ? 'soundList' : 'badgeList'} lazyLoadingList__list">${items.join('')}</ul>
    </div>`;
}

// A track page: hero without a tracks summary, plus the engagement bar. The
// hero's artwork nests a second .sc-artwork inside the first, as the real one
// does — the marker takes the outer box.
export function trackPage({
	uploader = 'uploader',
	uploaderName = 'Uploader',
	title = 'Single Track',
	purchase = null,
	hero = true,
	engagement = true,
	artwork = ART('hero', 't500x500'),
	// The hero's own genre tag, which sits in the title block without a tag
	// container of its own; `null` renders none.
	genre = null,
	// The like and repost buttons, which a track page renders in the
	// engagement bar below the hero rather than in it.
	liked = false,
	reposted = false,
	actions = true,
	// Verified accounts nest a badge inside the uploader link, as they do on a
	// playlist hero.
	uploaderBadge = true
}: TrackPageOptions = {}) {
	return `
    ${
			hero
				? `<div class="l-listen-hero">
      <div class="fullHero">
        <div class="listenArtworkWrapper">
          <div class="listenArtworkWrapper__artwork">
            <div class="image sc-artwork sc-artwork-40x"><span class="sc-artwork image__full" style="background-image: url(&quot;${esc(artwork)}&quot;);"></span></div>
          </div>
        </div>
        <div class="soundTitle">
          <h1 class="soundTitle__title"><a href="/${esc(uploader)}/single-track">${esc(title)}</a></h1>
          <h2 class="soundTitle__username"><a href="/${esc(uploader)}">${esc(uploaderName)}${uploaderBadge ? verifiedBadge() : ''}</a></h2>
          ${genreTag(genre)}
        </div>
        <button class="sc-button-more hero-more" type="button">More</button>
      </div>
    </div>`
				: ''
		}
    <div class="l-listen-wrapper">
      ${
				engagement
					? `<div class="listenEngagement">
        <button class="sc-button-more" type="button">More</button>
        ${engagementButtons({ liked, reposted, actions })}
        ${purchase ? `<a class="soundActions__purchaseLink sc-buylink" href="${esc(gateSc(purchase.url))}" aria-label="${esc(purchase.label)}">${esc(purchase.label)}</a>` : ''}
      </div>`
					: ''
			}
    </div>`;
}

// The signed-in track page, which SoundCloud draws in an iframe of its own
// (`soundcloud.com/n/<user>/<track>`) as a React app sharing no markup with
// the classic one: no .fullHero, no .listenEngagement, and MUI buttons whose
// only class is a hash of the stylesheet that built them. What it does carry
// is the accessibility tree the content script reads it by — a
// `section[aria-label="Track header"]`, an <h1>, and one outbound link.
//
// `render` this at `/n/<uploader>/<track>`, the path the frame really has.
export function v2TrackPage({
	uploader = 'uploader',
	uploaderName = 'Uploader',
	title = 'Single Track',
	purchase = null,
	// `null` renders no cover art at all.
	artwork = ART('hero', 't500x500'),
	// `null` renders no genre tag.
	genre = 'Baile Funk',
	// The heading carries the whole title in a `title` attribute beside the
	// text it may have had to cut; `false` renders it without one.
	titleAttr = true,
	// The link under the title. `false` renders the header without one, which
	// is what a layout that stopped naming the uploader would look like.
	uploaderLink = true,
	// The second header SoundCloud renders for the other breakpoint, with a
	// cart of its own. Both are replaced; only one is ever on screen.
	headers = 1
}: V2TrackPageOptions = {}) {
	const header = () => `
    <section class="MuiStack-root mui-1piq555" aria-label="Track header">
      <h1 class="MuiTypography-root MuiTypography-h1"${titleAttr ? ` title="${esc(title)}"` : ''}>${esc(title)}</h1>
      <div class="MuiStack-root">
        ${uploaderLink ? `<a class="MuiLink-root" href="https://soundcloud.com/${esc(uploader)}"><span>${esc(uploaderName)}</span></a>` : ''}
        ${genre ? `<a class="MuiLink-root" href="https://soundcloud.com/tags/${encodeURIComponent(genre)}"><span>${esc(genre)}</span></a>` : ''}
      </div>
      <div class="MuiBox-root">${artwork ? `<img src="${esc(artwork)}" alt="${esc(title)}">` : ''}</div>
      <div class="MuiStack-root mui-16ytee5">
        <button class="MuiButtonBase-root MuiIconButton-root MuiIconButton-sizeMedium mui-163v5oj" type="button" aria-label="Share"></button>
        <button class="MuiButtonBase-root MuiIconButton-root MuiIconButton-sizeMedium mui-163v5oj" type="button" aria-label="Copy link"></button>
        ${
					purchase
						? `<a aria-label="Buy This Track" title="${esc(purchase.label)}" class="" target="_blank" href="${esc(gateSc(purchase.url))}"><button class="MuiButtonBase-root MuiIconButton-root MuiIconButton-sizeMedium mui-163v5oj" type="button"></button></a>`
						: ''
				}
        <button class="MuiButtonBase-root MuiIconButton-root MuiIconButton-sizeMedium mui-163v5oj" type="button" aria-label="More actions"></button>
      </div>
    </section>`;

	return Array.from({ length: headers }, header).join('');
}

// The player bar. `href` is the title link's, exactly as SoundCloud renders
// it; `href: null` gives the bare <span> it renders for an ad, which names no
// track. `artist` is the uploader link SoundCloud puts above the title once
// the track has loaded; `null` renders none, which is the bar while loading.
export function playerBar({
	title = 'Playing Track',
	href = '/artist/title',
	artwork = ART('player', 't50x50'),
	artist = null
}: PlayerBarOptions = {}) {
	const link =
		href === null
			? `<span class="playbackSoundBadge__titleLink sc-truncate" title="${esc(title)}">${esc(title)}</span>`
			: `<a class="playbackSoundBadge__titleLink sc-truncate" href="${esc(href)}" title="${esc(title)}">${esc(title)}</a>`;

	const uploader =
		artist === null
			? ''
			: `<a class="playbackSoundBadge__lightLink sc-link-light sc-truncate" href="/${esc(artist.toLowerCase())}" title="${esc(artist)}">${esc(artist)}</a>`;

	return `
    <div class="playControls">
      <div class="playControls__inner">
        <div class="playbackSoundBadge">
          <div class="playbackSoundBadge__avatar sc-media-image">
            <div class="image sc-artwork sc-artwork-4x">${imageFull(artwork)}</div>
          </div>
          <div class="playbackSoundBadge__titleContextContainer">
            ${uploader}
            <div class="playbackSoundBadge__title">${link}</div>
          </div>
        </div>
      </div>
    </div>`;
}

// SoundCloud's More menu renders detached from its row, appended to <body>.
export function moreMenu() {
	return `
    <div class="moreActions">
      <button class="sc-button-download" type="button"><span class="label">Download file</span></button>
    </div>`;
}

// The server-rendered hydration script. Appended to <head> like SoundCloud's;
// jsdom parses but never executes it, as an isolated-world reader never
// would.
export function hydrationScript(entries: Json, { raw }: { raw?: string } = {}) {
	const script = document.createElement('script');
	script.textContent = raw ?? `window.__sc_hydration = ${JSON.stringify(entries)};`;
	document.head.append(script);

	return script;
}

// Puts markup on the page at `path` (the address bar) and returns the body.
export function render(html: string, path = '/') {
	history.replaceState(null, '', path);
	document.body.innerHTML = html;

	return document.body;
}

// ---- API-shaped objects -----------------------------------------------------

export function apiTrack({
	id = 1,
	permalink = '/artist/title',
	downloadable = false,
	hasDownloadsLeft = true,
	purchaseUrl = null,
	purchaseTitle = null,
	genre = null,
	// What SoundCloud names as the cover: the track's own artwork, else the
	// uploader's avatar, which is what its rows fall back to. `null` for both is
	// an API answer too — a track with neither.
	artworkUrl = null,
	avatarUrl = null,
	kind = 'track'
}: ApiTrackOptions = {}) {
	return {
		kind,
		id,
		permalink_url: `https://soundcloud.com${permalink}`,
		downloadable,
		has_downloads_left: hasDownloadsLeft,
		purchase_url: purchaseUrl,
		purchase_title: purchaseTitle,
		genre,
		artwork_url: artworkUrl,
		user: { avatar_url: avatarUrl },
		title: permalink.split('/').pop()
	};
}

export const CLIENT_ID = 'test-client-id';

export const apiClientEntry = (id = CLIENT_ID) => ({ hydratable: 'apiClient', data: { id } });
