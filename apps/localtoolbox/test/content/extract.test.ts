// content.ts — page detection and track extraction (the EXTRACT_TRACKS
// message), on every supported layout and on the pages it must ignore.
import { describe, it, expect, afterEach, beforeEach } from 'vitest';
import { loadContentScript, disposeContentScripts } from '../helpers/load.ts';
import {
	render,
	playlistPage,
	playlistRow,
	systemPlaylistPage,
	publicLikesPage,
	ownLikesPage,
	likedSound,
	likedTile,
	tilePlaceholder,
	trackPage,
	v2TrackPage,
	ART
} from '../helpers/dom.ts';
import type { ChromeMock } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

afterEach(disposeContentScripts);

const extract = async () => {
	await loadContentScript();

	return chrome.testSendMessage({ type: 'EXTRACT_TRACKS' });
};

describe('playlist pages', () => {
	it('reads title, curator, and every row of a regular playlist', async () => {
		render(
			playlistPage({
				title: '  Late   Night  ',
				curator: 'Curator',
				rows: [
					playlistRow({
						artist: 'Bicep',
						title: 'Glue',
						href: '/bicep/glue?in=curator/sets/late-night',
						artwork: ART('a'),
						liked: true,
						reposted: true
					}),
					playlistRow({
						artist: 'Four Tet',
						title: 'Baby',
						href: '/fourtet/baby/?in=curator/sets/late-night',
						artwork: ''
					})
				]
			}),
			'/curator/sets/late-night'
		);
		const response = await extract();
		expect(response).toMatchObject({
			ok: true,
			pageType: 'playlist',
			pageKey: '/curator/sets/late-night',
			pageDetectedBy: 'dom',
			title: 'Late Night',
			curator: 'Curator', // the nested "Verified" badge text is skipped
			skippedPlaylists: 0
		});
		// A playlist row renders no genre tag, so the genre is unknown from the
		// page — the hydration and the lookup are where it comes from there.
		expect(response.tracks).toEqual([
			{
				artist: 'Bicep',
				title: 'Glue',
				artworkUrl: ART('a'),
				permalink: '/bicep/glue',
				genre: null,
				liked: true,
				reposted: true
			},
			{
				artist: 'Four Tet',
				title: 'Baby',
				artworkUrl: '',
				permalink: '/fourtet/baby',
				genre: null,
				liked: false,
				reposted: false
			}
		]);
	});

	it('identifies the playlist from the rows even when the address bar disagrees', async () => {
		render(playlistPage({ rows: [playlistRow({ href: '/a/t?in=/other/sets/real-one' })] }), '/you/insights');
		const response = await extract();
		expect(response.pageType).toBe('playlist');
		expect(response.pageKey).toBe('/other/sets/real-one');
	});

	it('falls back to the URL for the playlist path when no row names one', async () => {
		render(
			playlistPage({ rows: [playlistRow({ href: '/a/t' }), playlistRow({ href: null })] }),
			'/curator/sets/unnamed/'
		);
		const response = await extract();
		expect(response.pageKey).toBe('/curator/sets/unnamed');
		expect(response.tracks.map((t) => t.permalink)).toEqual(['/a/t', '']);
	});

	it('skips rows whose title link is not a parsable URL', async () => {
		// An address bar that names no playlist, so the key can only have come
		// from the row after the broken one.
		render(
			playlistPage({
				rows: [
					playlistRow({ href: 'https://a b/broken?in=x/sets/y' }),
					playlistRow({ href: '/a/t?in=curator/sets/ok' })
				]
			}),
			'/you/insights'
		);
		const response = await extract();
		expect(response.pageKey).toBe('/curator/sets/ok');
		expect(response.tracks[0].permalink).toBe('');
	});

	it('reads system playlists (/discover/sets/…) with their own hero and rows', async () => {
		render(
			systemPlaylistPage({
				title: 'Daily Drops',
				curator: 'SoundCloud',
				rows: [
					playlistRow({
						system: true,
						artist: 'X',
						title: 'Y',
						href: '/x/y?in_system_playlist=soundcloud:system-playlists:123'
					})
				]
			}),
			'/discover/sets/daily-drops'
		);
		const response = await extract();
		expect(response).toMatchObject({
			pageType: 'playlist',
			pageKey: '/discover/sets/soundcloud:system-playlists:123',
			title: 'Daily Drops',
			curator: 'SoundCloud'
		});
		expect(response.tracks).toEqual([
			{ artist: 'X', title: 'Y', artworkUrl: ART('row'), permalink: '/x/y', genre: null, liked: false, reposted: false }
		]);
	});

	it('recognises a playlist by its rendered list when the hero has no summary', async () => {
		render(playlistPage({ summary: false, rows: [playlistRow()] }), '/c/sets/p');
		const response = await extract();
		expect(response.pageType).toBe('playlist');
		expect(response.tracks).toHaveLength(1);
	});

	it('recognises a system playlist by its details block without a hero', async () => {
		render(systemPlaylistPage({ hero: false, rows: [playlistRow({ system: true })] }), '/discover/sets/x');
		const response = await extract();
		expect(response.pageType).toBe('playlist');
		expect(response.title).toBe('');
		expect(response.curator).toBe('');
	});

	it('falls back to any .trackItem when the list wrappers are renamed, on playlist pages only', async () => {
		render(playlistPage({ rows: [playlistRow({ itemClass: 'renamed__item', artist: 'R', title: 'S' })] }), '/c/sets/p');
		const response = await extract();
		expect(response.tracks).toEqual([
			{
				artist: 'R',
				title: 'S',
				artworkUrl: ART('row'),
				permalink: '/artist/title',
				genre: null,
				liked: false,
				reposted: false
			}
		]);
	});

	it('drops rows with neither artist nor title', async () => {
		render(
			playlistPage({ rows: [playlistRow({ artist: '', title: '' }), playlistRow({ artist: '', title: 'Only' })] }),
			'/c/sets/p'
		);
		const response = await extract();
		expect(response.tracks.map((t) => t.title)).toEqual(['Only']);
	});

	it('reads the curator without a badge', async () => {
		render(playlistPage({ curatorBadge: false, curator: 'Plain', rows: [playlistRow()] }), '/c/sets/p');
		expect((await extract()).curator).toBe('Plain');
	});
});

describe('likes pages', () => {
	it('reads a public Likes page in the sound-list layout, skipping liked playlists', async () => {
		render(
			publicLikesPage({
				user: 'dj-someone',
				heading: 'Likes by DJ Someone',
				items: [
					likedSound({
						displayArtist: 'BARELY ALIVE',
						artistHref: '/barelyalive',
						title: 'Fire',
						href: '/barelyalive/fire',
						artwork: ART('s1', 't500x500'),
						purchase: { url: 'https://hypeddit.com/fire', label: 'Free Download' },
						// The tag SoundCloud renders beside the title, which is the
						// track's genre and the reason a Likes page needs no lookup to
						// have one.
						genre: '  Drum  &  Bass ',
						// Only the class says so — a redesign that drops aria-pressed.
						liked: 'class'
					}),
					likedSound({ playlist: true, title: 'A Playlist', href: '/x/sets/a-playlist' }),
					likedSound({ playlist: true, playlistClass: false, title: 'Unmarked Playlist', href: '/x/sets/unmarked' }),
					// Only aria-pressed says so, and the row renders no repost button
					// at all, which is not the same as not having reposted it.
					likedSound({
						usernameText: false,
						displayArtist: 'Fallback Name',
						title: 'Second',
						href: '/f/second',
						artwork: '',
						reposted: 'aria'
					}),
					likedSound({ artist: '', title: '', href: '/empty/row' })
				]
			}),
			'/dj-someone/likes'
		);
		const response = await extract();
		expect(response).toMatchObject({
			pageType: 'likes',
			pageKey: '/dj-someone/likes',
			pageDetectedBy: 'dom',
			title: 'Likes by DJ Someone',
			curator: '',
			skippedPlaylists: 2
		});
		expect(response.tracks).toEqual([
			{
				artist: 'BARELY ALIVE',
				title: 'Fire',
				artworkUrl: ART('s1', 't500x500'),
				permalink: '/barelyalive/fire',
				itemIndex: 0,
				domPurchaseUrl: 'https://hypeddit.com/fire',
				domPurchaseTitle: 'Free Download',
				genre: 'Drum & Bass',
				liked: true,
				reposted: false
			},
			{
				artist: 'Fallback Name',
				title: 'Second',
				artworkUrl: '',
				permalink: '/f/second',
				itemIndex: 3,
				domPurchaseUrl: '',
				domPurchaseTitle: '',
				// This row rendered no tag, which is not "no genre": nobody knows.
				genre: null,
				liked: false,
				reposted: true
			}
		]);
	});

	it('reads the signed-in user’s likes in the badge grid, skipping playlist tiles and placeholders', async () => {
		render(
			ownLikesPage({
				items: [
					likedTile({ artist: 'Uploader', title: 'Tile One', href: '/u/tile-one', artwork: ART('t1', 't500x500') }),
					likedTile({ title: 'Playlist Tile', href: '/u/sets/pl', artworkHref: '/u/sets/pl' }),
					likedTile({ title: 'Playlist By Art Link', href: '/u/heading', artworkHref: '/u/sets/pl2' }),
					tilePlaceholder(),
					likedTile({ heading: false, artist: 'Art Only', artworkHref: '/u/art-only', artwork: '' }),
					likedTile({ icon: false, title: 'No Icon', href: '/u/no-icon' }),
					likedTile({ artist: '', heading: false, artworkHref: '/u/nothing' })
				]
			}),
			'/you/likes'
		);
		const response = await extract();
		expect(response).toMatchObject({
			pageType: 'likes',
			pageKey: '/you/likes',
			title: 'your likes',
			skippedPlaylists: 2
		});
		// A tile renders no tag and neither row button, so the genre, the like
		// and the repost are all null — "nobody knows", not "no".
		expect(response.tracks).toEqual([
			{
				artist: 'Uploader',
				title: 'Tile One',
				artworkUrl: ART('t1', 't500x500'),
				permalink: '/u/tile-one',
				itemIndex: 0,
				domPurchaseUrl: '',
				domPurchaseTitle: '',
				genre: null,
				liked: null,
				reposted: null
			},
			{
				artist: 'Art Only',
				title: '',
				artworkUrl: '',
				permalink: '/u/art-only',
				itemIndex: 3, // grid placeholders are not liked items
				domPurchaseUrl: '',
				domPurchaseTitle: '',
				genre: null,
				liked: null,
				reposted: null
			},
			{
				artist: 'Uploader',
				title: 'No Icon',
				artworkUrl: ART('tile', 't500x500'),
				permalink: '/u/no-icon',
				itemIndex: 4,
				domPurchaseUrl: '',
				domPurchaseTitle: '',
				genre: null,
				liked: null,
				reposted: null
			}
		]);
	});

	it('recognises /you/likes from the Library tab bar when it is rendered', async () => {
		// Without the view toggle, which would say /you/likes on its own.
		render(ownLikesPage({ tabs: true, toggle: false, list: true, items: [likedSound()] }), '/you/insights');
		const response = await extract();
		expect(response).toMatchObject({ pageType: 'likes', pageKey: '/you/likes', title: 'your likes' });
		expect(response.tracks).toHaveLength(1);
	});

	it('ignores active tabs that do not point at a Likes page', async () => {
		render(
			'<ul class="g-tabs"><li><a class="g-tabs-link active" href="/you/library">Overview</a></li></ul><p>Library</p>',
			'/you/library'
		);
		const response = await extract();
		expect(response).toMatchObject({ pageType: 'other', pageKey: '', pageDetectedBy: 'url', tracks: [] });
	});

	it('leaves the title empty on a public Likes page without a heading', async () => {
		render(publicLikesPage({ heading: '', items: [likedSound()] }).replace(/<h1[^>]*>.*?<\/h1>/, ''), '/dj/likes');
		const response = await extract();
		expect(response.title).toBe('');
		expect(response.tracks).toHaveLength(1);
	});

	it('ignores the nested track preview of a liked playlist', async () => {
		render(publicLikesPage({ items: [likedSound({ playlist: true })] }), '/dj/likes');
		const response = await extract();
		expect(response.tracks).toEqual([]);
		expect(response.skippedPlaylists).toBe(1);
	});

	it('reads a purchase link only when its address parses', async () => {
		render(
			publicLikesPage({
				items: [
					likedSound({ href: '/a/one', purchase: { url: 'https://hypeddit.com/one', label: 'Free Download' } }),
					likedSound({ href: '/a/two', purchase: { url: 'https://direct.test/two', label: 'Buy' } }),
					likedSound({ href: '/a/three', purchase: { url: 'https://direct.test/three', label: 'Buy' } })
				]
			}),
			'/dj/likes'
		);
		const links = document.querySelectorAll('.soundActions__purchaseLink');
		links[1].setAttribute('href', 'https://direct.test/two'); // not wrapped in gate.sc
		links[2].setAttribute('href', 'https://a b/broken');

		const gateWithoutUrl = likedSound({ href: '/a/four', purchase: { url: 'x', label: 'Buy' } }).replace(
			/href="https:\/\/gate\.sc[^"]*"/,
			'href="https://gate.sc?token=only"'
		);

		document.querySelector('.soundList')!.insertAdjacentHTML('beforeend', gateWithoutUrl);
		const response = await extract();
		expect(response.tracks.map((t) => t.domPurchaseUrl)).toEqual([
			'https://hypeddit.com/one',
			'https://direct.test/two',
			'',
			''
		]);
	});

	it('records a row whose title is not a link', async () => {
		render(publicLikesPage({ items: [likedSound({ href: null, title: 'Linkless' })] }), '/dj/likes');
		const response = await extract();
		expect(response.tracks[0]).toMatchObject({ title: 'Linkless', permalink: '' });
	});
});

describe('track pages', () => {
	it('reads the one track a song page is about, from its hero and engagement bar', async () => {
		render(
			trackPage({
				uploader: 'uploader',
				uploaderName: 'Uploader',
				title: '  Single   Track ',
				genre: 'Techno',
				liked: true,
				purchase: { url: 'https://hypeddit.com/x', label: 'Free Download' }
			}),
			'/uploader/single-track'
		);
		const response = await extract();
		expect(response).toMatchObject({
			pageType: 'track',
			pageKey: '/uploader/single-track',
			pageDetectedBy: 'dom',
			title: 'Single Track',
			curator: 'Uploader',
			skippedPlaylists: 0
		});
		expect(response.tracks).toEqual([
			{
				title: 'Single Track',
				artist: 'Uploader',
				artworkUrl: ART('hero', 't500x500'),
				genre: 'Techno',
				// Read from the engagement bar below the hero, not from the hero.
				liked: true,
				reposted: false,
				permalink: '/uploader/single-track',
				itemIndex: 0,
				domPurchaseUrl: 'https://hypeddit.com/x',
				domPurchaseTitle: 'Free Download'
			}
		]);
	});

	it('reports no purchase link, and no tag, where the page renders neither', async () => {
		render(trackPage(), '/uploader/single-track');
		const response = await extract();
		expect(response.tracks[0]).toMatchObject({
			genre: null,
			domPurchaseUrl: '',
			domPurchaseTitle: '',
			liked: false,
			reposted: false
		});
	});

	// The legacy class on its own, and no aria-label — the link's own text is
	// the label then, as it is the only wording the page carries.
	it('reads a purchase link that carries only the legacy class and its text', async () => {
		render(trackPage({ purchase: { url: 'https://buy.test', label: 'Buy' } }), '/uploader/single-track');
		const link = document.querySelector('.listenEngagement a.soundActions__purchaseLink')!;
		link.className = 'sc-buylink';
		link.removeAttribute('aria-label');
		const response = await extract();
		expect(response.tracks[0]).toMatchObject({
			domPurchaseUrl: 'https://buy.test',
			domPurchaseTitle: 'Buy'
		});
	});

	// detectPage refuses a URL the hero disagrees with, and the track is still
	// reported — exactly as a Likes row whose title is not a link is. It has no
	// identity, so nothing that holds a list can hold it.
	it('reports the track with no permalink when the URL and the hero disagree', async () => {
		render(trackPage({ uploader: 'uploader' }), '/someone-else/single-track');
		const response = await extract();
		expect(response).toMatchObject({ pageType: 'track', pageKey: '' });
		expect(response.tracks[0]).toMatchObject({ title: 'Single Track', permalink: '' });
	});

	// Read off a live track page: the uploader link nests a "Verified" badge
	// whose SVG <title> is text, and a name read through textContent carries it.
	it('names the uploader without the Verified badge nested in the link', async () => {
		render(trackPage({ uploaderName: 'kstko', uploaderBadge: true }), '/uploader/single-track');
		const response = await extract();
		expect(response.tracks[0].artist).toBe('kstko');
		expect(response.curator).toBe('kstko');
	});

	it('has no track to report while the hero has not rendered', async () => {
		render(trackPage({ hero: false }), '/uploader/single-track');
		const response = await extract();
		expect(response).toMatchObject({ pageType: 'track', tracks: [], title: '' });
	});

	// A hero that drew its frame but none of its words names nothing, and an
	// entry of two empty strings is worse than saying so.
	it('has no track to report from a hero that names neither title nor artist', async () => {
		render(trackPage({ title: '', uploaderName: '' }), '/uploader/single-track');
		const response = await extract();
		expect(response.tracks).toEqual([]);
	});
});

describe('other pages', () => {
	it('falls back to the URL when nothing recognisable is rendered', async () => {
		render('<div class="loading"></div>', '/curator/sets/still-loading');
		expect(await extract()).toMatchObject({
			pageType: 'playlist',
			pageKey: '/curator/sets/still-loading',
			pageDetectedBy: 'url',
			tracks: []
		});

		render('<div class="loading"></div>', '/someone/likes/');
		expect(await extract()).toMatchObject({
			pageType: 'likes',
			pageKey: '/someone/likes',
			pageDetectedBy: 'url',
			title: ''
		});

		render('<div class="loading"></div>', '/you/likes');
		expect(await extract()).toMatchObject({ pageType: 'likes', pageKey: '/you/likes', title: 'your likes' });

		render('<div class="loading"></div>', '/stream');
		expect(await extract()).toMatchObject({ pageType: 'other', pageKey: '', pageDetectedBy: 'url' });
	});

	it('does not treat a profile’s embedded rows as the page’s tracks', async () => {
		render(
			`<div class="profileHeader">Profile</div>
       <ul><li class="soundList__item"><div>${likedSound({ title: 'Profile row' })
					.replace(/^[\s\S]*?<div class="sound/, '<div class="sound')
					.replace(/<\/div>\s*<\/li>\s*$/, '')}</div></li></ul>
       <div class="trackItem"><a class="trackItem__trackTitle" href="/p/t">Preview</a></div>`,
			'/someone'
		);
		const response = await extract();
		expect(response.pageType).toBe('other');
		expect(response.tracks).toEqual([]);
	});

	it('answers nothing to messages it does not understand', async () => {
		render('<p></p>', '/');
		await loadContentScript();
		expect(await chrome.testSendMessage({ type: 'SOMETHING_ELSE' })).toBeUndefined();
		expect(await chrome.testSendMessage(null)).toBeUndefined();
	});
});

describe('helpers, through the DOM', () => {
	beforeEach(() => {
		history.replaceState(null, '', '/c/sets/p');
	});

	it('collapses whitespace in every text field', async () => {
		render(
			playlistPage({ title: 'A\n\n  B', curator: ' C\tD ', rows: [playlistRow({ artist: ' X  Y ', title: '\nZ\n' })] })
		);
		const response = await extract();
		expect(response).toMatchObject({ title: 'A B', curator: 'C D' });
		expect(response.tracks[0]).toMatchObject({ artist: 'X Y', title: 'Z' });
	});

	it('reads artwork URLs from single-quoted and unquoted background images', async () => {
		render(playlistPage({ rows: [playlistRow(), playlistRow()] }));
		const spans = document.querySelectorAll('.trackItem span.image__full');
		spans[0].setAttribute('style', "background-image: url('https://i1.sndcdn.com/artworks-q-t120x120.jpg')");
		spans[1].setAttribute('style', 'background-image: url(https://i1.sndcdn.com/artworks-u-t120x120.jpg)');
		const response = await extract();
		expect(response.tracks.map((t) => t.artworkUrl)).toEqual([ART('q'), ART('u')]);
	});

	it('reports no artwork for a data: background or a missing image element', async () => {
		render(playlistPage({ rows: [playlistRow(), playlistRow()] }));
		const spans = document.querySelectorAll('.trackItem span.image__full');
		spans[0].setAttribute('style', 'background-image: url(data:image/gif;base64,R0lGOD)');
		spans[1].remove();
		const response = await extract();
		expect(response.tracks.map((t) => t.artworkUrl)).toEqual(['', '']);
	});

	it('accepts absolute permalinks and strips trailing slashes', async () => {
		render(playlistPage({ rows: [playlistRow({ href: 'https://soundcloud.com/a/b/?in=c/sets/p' })] }));
		expect((await extract()).tracks[0].permalink).toBe('/a/b');
	});
});

// ---- Which document answers -------------------------------------------------

describe('the frames the script also runs in', () => {
	// The manifest says `all_frames`, because the signed-in track page is an
	// iframe of SoundCloud's own and its buttons have to be replaced there too.
	// A panel asks its question with `chrome.tabs.sendMessage` and no frame id,
	// which asks every frame in the tab and keeps whichever answers first — so
	// only the tab's own document may answer, or the panel would sometimes read
	// the page and sometimes read the frame.
	afterEach(() => {
		Object.defineProperty(window, 'top', { value: window, configurable: true });
	});

	it('does not answer the panels from inside a frame', async () => {
		Object.defineProperty(window, 'top', { value: {}, configurable: true });
		render(playlistPage({ rows: [playlistRow()] }), '/c/sets/p');
		await loadContentScript();

		expect(chrome.runtime.onMessage.addListener).not.toHaveBeenCalled();
		expect(await chrome.testSendMessage({ type: 'EXTRACT_TRACKS' })).toBeUndefined();
	});

	// The frame is where the signed-in track page is, so the work on the page
	// itself has to happen there — only the answering is the top's.
	it('still does the page’s own work from inside a frame', async () => {
		Object.defineProperty(window, 'top', { value: {}, configurable: true });
		render(
			v2TrackPage({ uploader: 'y3llo', purchase: { url: 'https://hypeddit.com/x', label: 'DOWNLOAD' } }),
			'/n/y3llo/taw-gawli'
		);
		await loadContentScript();

		expect(document.querySelector('.tbx-dl')).not.toBe(null);
		expect(document.querySelector('.tbx-dl-replaced')).not.toBe(null);
	});

	it('answers them from the page itself', async () => {
		render(playlistPage({ rows: [playlistRow()] }), '/c/sets/p');
		await loadContentScript();

		expect((await chrome.testSendMessage({ type: 'EXTRACT_TRACKS' })).tracks).toHaveLength(1);
	});
});
