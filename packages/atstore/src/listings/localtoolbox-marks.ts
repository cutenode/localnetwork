// The page localtoolbox's marks are shown on: the made-up playlist (localtoolbox-page.ts) laid out
// as a playlist page, in the markup the content script's own tests build
// (apps/localtoolbox/test/helpers/dom.ts), under a plain stylesheet of this file's: the layout of
// SoundCloud's playlist pages, none of its branding.
import { playerBar, playlistPage, playlistRow } from '../../../../apps/localtoolbox/test/helpers/dom.ts';
import { ARTWORK_HOST, PAGE } from './localtoolbox-page.ts';

const { EXTRACT_TRACKS: playlist, GET_DOWNLOAD_DETAILS: looked } = PAGE.answers;

/** The tracks marked downloaded, by their place in the playlist. The others are marked as their details say. */
const DOWNLOADED = [0, 3, 4, 7];

/** The track in the player bar. */
const PLAYING = 0;

/**
 * What chrome.storage.local holds as the page opens: the downloaded marks, and every track's
 * details as if looked up `now`, which the content script only trusts while they're fresh.
 */
export function marksStorage(now: number) {
	const at = new Date(now).toISOString();

	return {
		downloaded: Object.fromEntries(DOWNLOADED.map((i) => [playlist.tracks[i].permalink, at])),
		details: Object.fromEntries(looked.tracks.map((track) => [track.permalink, { ...track.details, at: now }]))
	};
}

const STYLE = `
* { box-sizing: border-box; }
html { background: #f2f2f2; }
body { margin: 0; color: #333; font: 14px/1.4 system-ui, sans-serif; }
a { color: inherit; text-decoration: none; }
button { font: inherit; color: inherit; cursor: pointer; }
.sc-artwork { display: block; overflow: hidden; background: #ddd; }
.image__full { display: block; background-size: cover; background-position: center; }

.top { position: sticky; top: 0; z-index: 5; height: 46px; background: #262626; color: #ccc; }
.top__inner { display: flex; align-items: center; gap: 22px; max-width: 1240px; height: 100%; margin: 0 auto; padding: 0 16px; }
.top nav { display: flex; gap: 22px; font-weight: 600; }
.top nav a:first-child { color: #fff; }
.search { flex: 1; max-width: 460px; padding: 6px 10px; border-radius: 4px; background: #e5e5e5; color: #999; }
.top .face { width: 26px; height: 26px; margin-left: auto; border-radius: 50%; }

.page { display: grid; grid-template-columns: minmax(0, 1fr) 300px; align-content: start; column-gap: 32px; max-width: 1240px; min-height: calc(100vh - 46px); margin: 0 auto; padding: 0 30px 70px; background: #fff; }
.l-listen-hero { grid-column: 1 / -1; margin: 0 -30px; }
.fullHero { position: relative; display: flex; justify-content: space-between; height: 290px; padding: 30px; background: linear-gradient(135deg, #3c3552, #8a5f4c); }
.fullHero__title { padding-left: 76px; }
.fullHero__title::before { content: ''; position: absolute; left: 30px; top: 30px; width: 60px; height: 60px; border-radius: 50%; background: #111 url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'%3E%3Cpath d='M9 7v10l8-5z' fill='white'/%3E%3C/svg%3E") center / 34px no-repeat; }
.soundTitle__title, .soundTitle__username { margin: 0; font-weight: 400; }
.soundTitle__title span, .soundTitle__username a { display: inline-block; padding: 4px 8px; background: rgb(0 0 0 / 0.85); }
.soundTitle__title span { color: #fff; font-size: 24px; line-height: 30px; }
.soundTitle__username { margin-top: 8px; font-size: 16px; }
.soundTitle__username a { color: #ccc; }
.verifiedBadge { display: inline-block; width: 14px; height: 14px; margin-left: 6px; vertical-align: -1px; border-radius: 50%; background: #38d; }
.verifiedBadge svg { display: none; }
.fullHero__tracksSummary { position: absolute; left: 106px; bottom: 30px; padding: 4px 10px; border-radius: 3px; background: rgb(0 0 0 / 0.6); color: #fff; font-size: 12px; text-transform: uppercase; }
.listenArtworkWrapper { order: 1; }
.listenArtworkWrapper__artwork .sc-artwork { width: 230px; height: 230px; }
.listenArtworkWrapper__artwork .image__full { width: 100%; height: 100%; }

.listenEngagement { display: flex; gap: 8px; padding: 12px 0; border-bottom: 1px solid #f2f2f2; }
.listenEngagement button { padding: 3px 10px; border: 1px solid #e5e5e5; border-radius: 3px; background: #fff; font-size: 12px; }
.trackList__list { margin: 0; padding: 0; list-style: none; counter-reset: row; }
.trackList__item { display: flex; align-items: center; gap: 10px; height: 44px; border-bottom: 1px solid #f2f2f2; counter-increment: row; }
.trackList__item::before { content: counter(row); width: 18px; color: #999; font-size: 12px; text-align: right; }
.trackItem { display: flex; flex: 1; align-items: center; gap: 10px; min-width: 0; }
.trackItem__image .sc-artwork { width: 34px; height: 34px; }
.trackItem__image .image__full { width: 100%; height: 100%; }
.trackItem__content { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.trackItem__username { color: #999; }
.trackItem__username::after { content: ' \\2013 '; }
.trackItem__actions { display: flex; gap: 4px; margin-left: auto; }
.trackItem__actions button { width: 26px; height: 22px; padding: 0; border: 1px solid #e5e5e5; border-radius: 3px; background: #fff; font-size: 0; }
.trackItem__actions button::before { font-size: 11px; color: #666; }
.sc-button-like::before { content: '\\2665'; }
.sc-button-repost::before { content: '\\21c4'; }
.sc-button-more::before { content: '\\2022\\2022\\2022'; }
.listenEngagement .sc-button-more { font-size: 0; }
.listenEngagement .sc-button-more::before { font-size: 12px; }
.l-sidebar { display: none; }

.side { padding-top: 20px; color: #999; font-size: 12px; }
.side h3 { margin: 0 0 10px; padding-bottom: 6px; border-bottom: 1px solid #f2f2f2; font-size: 14px; font-weight: 400; }
.side section { margin-bottom: 24px; }
.user { display: grid; grid-template-columns: 50px 1fr; column-gap: 12px; align-items: center; }
.user img { grid-row: span 2; width: 50px; height: 50px; border-radius: 50%; }
.user b { color: #333; font-size: 14px; font-weight: 600; }
.faces { display: flex; gap: 6px; }
.faces img { width: 32px; height: 32px; border-radius: 50%; }

.playControls { position: fixed; inset: auto 0 0; z-index: 5; height: 48px; border-top: 1px solid #ccc; background: #f2f2f2; }
.playControls__inner { display: flex; align-items: center; gap: 16px; max-width: 1240px; height: 100%; margin: 0 auto; padding: 0 16px; }
.playControls__inner::before { content: '\\23EE\\00a0\\00a0\\25B6\\00a0\\00a0\\23ED'; order: -2; color: #333; font-size: 16px; }
.playControls__inner::after { content: ''; flex: 1; order: -1; height: 2px; background: linear-gradient(to right, #333 38%, #ccc 38%); }
.playbackSoundBadge { display: flex; align-items: center; gap: 10px; width: 300px; font-size: 12px; }
.playbackSoundBadge .sc-artwork { width: 30px; height: 30px; }
.playbackSoundBadge .image__full { width: 100%; height: 100%; }
.playbackSoundBadge__titleContextContainer { display: grid; min-width: 0; }
.playbackSoundBadge__lightLink { color: #999; }
.sc-truncate { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }

@media (max-width: 640px) {
	.top__inner { gap: 12px; }
	.top nav a:not(:first-child) { display: none; }
	.page { display: block; padding: 0 12px 64px; }
	.l-listen-hero { margin: 0 -12px; }
	.fullHero { height: 170px; padding: 16px; }
	.fullHero__title { padding: 0 12px 0 52px; }
	.fullHero__title::before { left: 16px; top: 16px; width: 40px; height: 40px; background-size: 24px; }
	.soundTitle__title span { font-size: 18px; line-height: 24px; }
	.soundTitle__username { font-size: 14px; }
	.fullHero__tracksSummary { left: 68px; bottom: 16px; }
	.listenArtworkWrapper__artwork .sc-artwork { width: 120px; height: 120px; }
	.trackItem__actions button:not(.sc-button-more) { display: none; }
	.side { display: none; }
	.playbackSoundBadge { width: auto; flex: 1; }
	.playControls__inner::after { display: none; }
}
`;

/** The playlist page, whole: a bar, the playlist, a sidebar about its curator, and the player bar. */
export function marksPage(): string {
	const setPath = playlist.pageKey.slice(1);
	const avatar = (name: string) => `${ARTWORK_HOST}avatars-${name}-t200x200.jpg`;

	const rows = playlist.tracks.map((track) =>
		playlistRow({
			artist: track.artist,
			title: track.title,
			href: `${track.permalink}?in=${setPath}`,
			artwork: track.artworkUrl
		})
	);

	const playing = playlist.tracks[PLAYING];

	return `<!doctype html>
<html lang="en">
<meta charset="utf-8">
<title>${playlist.title} | Playlists</title>
<style>${STYLE}</style>
<body>
<header class="top"><div class="top__inner">
	<nav><a href="/discover">Home</a><a href="/feed">Feed</a><a href="/you/library">Library</a></nav>
	<div class="search">Search</div>
	<nav><a href="/upload">Upload</a></nav>
	<img class="face" src="${avatar('listener')}" alt="">
</div></header>
<main class="page">
	${playlistPage({
		title: playlist.title,
		curator: playlist.curator,
		curatorHref: '/halflight',
		setHref: playlist.pageKey,
		rows
	})}
	<aside class="side">
		<section>
			<h3>Curated by</h3>
			<div class="user"><img src="${avatar('halflight')}" alt=""><b>${playlist.curator}</b><span>12.4K followers · 86 tracks</span></div>
		</section>
		<section>
			<h3>214 likes</h3>
			<div class="faces">${['ada', 'bo', 'cy', 'dee', 'eli', 'fen', 'gus'].map((name) => `<img src="${avatar(name)}" alt="">`).join('')}</div>
		</section>
	</aside>
</main>
${playerBar({ title: playing.title, href: playing.permalink, artwork: playing.artworkUrl, artist: playing.artist })}
</body>
</html>`;
}
