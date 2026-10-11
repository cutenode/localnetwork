// The exact bytes of everything the extension hands over.
//
// Every other test asks whether the output is right in some respect: that the
// rows are in order, that a comma is quoted, that the name has no slash in it.
// These ask whether it is the same — the whole string, character for
// character, for one fixture chosen to be as awkward as a real playlist gets:
// a comma, a quotation mark, a newline, an accent, a dash inside a title, a
// duplicate, a name full of characters Windows will not have.
//
// A snapshot is not an argument for the output being correct; the tests beside
// it are. What it is, is a change nobody can make by accident: paste a file
// somebody exported last year into the panel and it still reads, because this
// says the format did not move.
import { describe, it, expect, beforeEach } from 'vitest';
import type { ArtistEntry, ListedEntry, QueueEntry } from '$lib/types';
import { loadState, type StateTransfer } from './helpers/load.ts';
import { createChrome } from './helpers/chrome.ts';
import { FORMATS, formatFor, processTracks, slugify } from '../src/tools/soundcloud-playlist-to-text/formats.ts';
import { safeName, filenameOf } from '../src/tools/third-party-downloads/save.ts';

// One fixture, chosen to break every format in a different way.
const TRACKS = [
	{ artist: 'Bicep', title: 'Glue' },
	{ artist: 'Channel Uploads', title: 'Justin Timberlake — SexyBack (EDIT)' },
	{ artist: 'Björk', title: 'Jóga, "Live"' },
	{ artist: 'Bicep', title: 'Glue' },
	{ artist: 'Two\nLines', title: 'Tab\there' }
];

describe('the text formats', () => {
	it.each(FORMATS.map((format) => [format.value, format]))('renders %s', (_value, format) => {
		expect(format.render(TRACKS)).toMatchSnapshot();
	});

	it('renders with the artist split out of the title', () => {
		expect(formatFor('artist-title').render(processTracks(TRACKS, { splitTitle: true }))).toMatchSnapshot();
	});

	it('renders deduped, and with both options at once', () => {
		expect(formatFor('numbered').render(processTracks(TRACKS, { dedupe: true }))).toMatchSnapshot('numbered, deduped');
		expect(formatFor('csv').render(processTracks(TRACKS, { splitTitle: true, dedupe: true }))).toMatchSnapshot(
			'csv, split and deduped'
		);
	});

	it('names the downloaded file after the page', () => {
		expect(
			['My Playlist', 'your likes', 'Likes by DJ Someone', 'Ø / Ambient — 2026', '', '中文播放列表'].map(
				(title) => `${slugify(title)}.${formatFor('csv').ext}`
			)
		).toMatchSnapshot();
	});
});

describe('the names files are saved under', () => {
	it('names an unlocked download after what the link says it is', () => {
		expect(
			[
				'https://gates.s3.amazonaws.com/886rp1?response-content-disposition=attachment%3B%20filename%3D%22KAKU%20-%20FE!N%20(Remix).wav%22',
				'https://cdn.example.com/tracks/my%20song.mp3',
				'https://cdn.example.com/tracks/no-extension',
				'https://cdn.example.com/a/../../etc/passwd.mp3',
				'not a url'
			].map(filenameOf)
		).toMatchSnapshot();
	});

	it('makes any name into one Chrome will take', () => {
		expect(
			[
				'../../etc/passwd',
				'.hidden',
				'a/b\\c',
				'tab\there',
				'x'.repeat(300),
				'Smal\u00e9 - Lose My Breath (baile flip)[Wile Out] *FREE DL*.mp3',
				'Remix? "Radio" Edit <v2>|final:.mp3',
				'zero\u200bwidth\u00adsoft.mp3',
				'trailing dots...',
				'NUL.mp3'
			].map(safeName)
		).toMatchSnapshot();
	});
});

describe('the exported state file', () => {
	let TbxState: StateTransfer;
	beforeEach(async () => {
		TbxState = await loadState();
	});

	// Not a snapshot: nearly all of the file is what it was handed, which a
	// snapshot of it would only say back. What is asserted is that it carries
	// all of that untouched, in the members' own order, with the few things it
	// works out for itself — what it is, when it was written, and the panels'
	// options read out of localStorage's strings — stated rather than recorded.
	it('carries what it was given, works out the rest, and reads back the same', async () => {
		const storage = {
			downloaded: {
				'/bicep/glue': '2026-01-02T03:04:05.678Z',
				'/björk/jóga': '2025-12-31T23:59:59.000Z'
			},
			details: {
				'/bicep/glue': {
					id: 1,
					permalink: '/bicep/glue',
					downloadable: true,
					hasDownloadsLeft: false,
					purchaseUrl: 'https://hypeddit.com/886rp1',
					purchaseTitle: 'Free Download',
					genre: 'Drum & Bass',
					at: 1757332800000
				},
				'/nobody/nothing': { unavailable: true, at: 1757332800000 }
			}
		};

		globalThis.chrome = createChrome({ storage });
		localStorage.setItem('pref:background', '1');
		localStorage.setItem('pref:sort', 'page');
		localStorage.setItem('pref:hide-downloaded', '0');

		// A queue with one of each kind of entry: a track with the details
		// SoundCloud gave and everything its page said about it, and one with
		// only what a Likes row rendered, queued from a layout that said none of
		// the three. The panel hands this in from the store that owns it, which
		// is why nothing here is in session storage.
		const queue: QueueEntry[] = [
			{
				permalink: '/bicep/glue',
				title: 'Glue',
				artist: 'Bicep',
				artworkUrl: 'https://i1.sndcdn.com/artworks-glue-t120x120.jpg',
				details: {
					id: 1,
					permalink: '/bicep/glue',
					downloadable: true,
					hasDownloadsLeft: false,
					purchaseUrl: 'https://hypeddit.com/886rp1',
					purchaseTitle: 'Free Download',
					genre: 'Drum & Bass',
					at: 1757332800000
				},
				genre: 'Drum & Bass',
				liked: true,
				reposted: false
			},
			{
				permalink: '/björk/jóga',
				title: 'Jóga, "Live"',
				artist: 'Björk',
				artworkUrl: 'https://i1.sndcdn.com/artworks-joga-t120x120.jpg',
				details: {
					id: null,
					permalink: '/björk/jóga',
					downloadable: null,
					hasDownloadsLeft: null,
					purchaseUrl: 'https://ffm.to/joga',
					purchaseTitle: '',
					genre: null,
					partial: true
				},
				genre: null,
				liked: null,
				reposted: null
			}
		];

		// The Manager's own list, handed in the same way: one track carrying
		// everything the list keeps, and one that came in from the queue on a tab
		// nobody could look anything up on.
		const listed: ListedEntry[] = [
			{
				permalink: '/bicep/glue',
				title: 'Glue',
				artist: 'Bicep',
				artworkUrl: 'https://i1.sndcdn.com/artworks-glue-t120x120.jpg',
				details: {
					id: 1,
					permalink: '/bicep/glue',
					downloadable: true,
					hasDownloadsLeft: false,
					purchaseUrl: 'https://hypeddit.com/886rp1',
					purchaseTitle: 'Free Download',
					genre: 'Drum & Bass',
					at: 1757332800000
				},
				genre: 'Drum & Bass',
				liked: true,
				reposted: false,
				domPurchaseTitle: 'Free Download',
				from: 'Late Night Cuts',
				at: 1757332800000
			},
			{
				permalink: '/björk/jóga',
				title: 'Jóga, "Live"',
				artist: 'Björk',
				artworkUrl: 'https://i1.sndcdn.com/artworks-joga-t120x120.jpg',
				details: null,
				genre: null,
				liked: null,
				reposted: null,
				domPurchaseTitle: '',
				from: 'the session queue',
				at: 1757332800000
			}
		];

		// The artists tool's list, handed in the same way: one artist SoundCloud
		// described, links and all, one it had no profile for, and one nobody has
		// asked about yet.
		const artists: ArtistEntry[] = [
			{
				permalink: '/bicep',
				name: 'Bicep',
				tracks: [{ permalink: '/bicep/glue', title: 'Glue' }],
				profile: {
					id: 1234,
					permalink: '/bicep',
					username: 'Bicep',
					fullName: 'Andy & Matt',
					city: 'Belfast',
					country: 'GB',
					followers: 123456,
					trackCount: 42,
					verified: true,
					avatarUrl: 'https://i1.sndcdn.com/avatars-bicep-large.jpg',
					bio: 'bookings: agent@example.com\nhttps://linktr.ee/bicep',
					links: [
						{ network: 'instagram', title: 'Instagram', url: 'https://www.instagram.com/bicepmusic/' },
						{ network: 'personal', title: 'feelmybicep.com', url: 'https://feelmybicep.com' }
					],
					at: 1757332800000
				},
				from: 'Late Night Cuts',
				at: 1757332800000
			},
			{
				permalink: '/nobody',
				name: 'Nobody',
				tracks: [{ permalink: '/nobody/nothing', title: '' }],
				profile: { unavailable: true, at: 1757332800000 },
				from: 'the session queue',
				at: 1757332800000
			},
			{
				permalink: '/björk',
				name: 'Björk',
				tracks: [{ permalink: '/björk/jóga', title: 'Jóga, "Live"' }],
				profile: null,
				from: 'the Manager’s list',
				at: 1757332800000
			}
		];

		const file = TbxState.serialize(await TbxState.snapshot(queue, listed, artists));
		const doc = JSON.parse(file);
		// The order the schema declares them in, which is the order two exports
		// are compared by when somebody diffs them.
		expect(Object.keys(doc)).toEqual([
			'format',
			'version',
			'exportedAt',
			'downloaded',
			'details',
			'listed',
			'artists',
			'queue',
			'prefs'
		]);
		expect(doc).toEqual({
			format: 'toolbox.manager.state',
			version: 1,
			// The clock test/setup.js freezes every test at.
			exportedAt: '2026-09-08T12:00:00.000Z',
			downloaded: storage.downloaded,
			details: storage.details,
			listed,
			artists,
			queue,
			// '1', 'page' and '0' as localStorage holds them.
			prefs: { background: true, sort: 'page', hideDownloaded: false }
		});
		// Laid out as plain two-space JSON, with nothing escaped that need not be
		// (the accents and the curly apostrophe are written as themselves).
		expect(file).toBe(JSON.stringify(doc, null, 2));
		expect(file).toContain('"/björk/jóga"');

		// What a later version has to keep being able to read.
		expect(TbxState.parse(file)).toEqual(doc);
	});

	it('says the same thing about every document it refuses', () => {
		const refusals = [
			'',
			'not json',
			'[]',
			'{}',
			'{"format":"something.else","version":1}',
			'{"format":"toolbox.manager.state","version":2}',
			'{"format":"toolbox.manager.state","version":1,"extra":1}',
			'{"format":"toolbox.manager.state","version":1,"exportedAt":"whenever"}',
			'{"format":"toolbox.manager.state","version":1,"downloaded":{"bad":"2026-01-01T00:00:00Z"}}',
			'{"format":"toolbox.manager.state","version":1,"downloaded":{"/a/b":"whenever"}}',
			'{"format":"toolbox.manager.state","version":1,"details":{"/a/b":{"unavailable":false,"at":1}}}',
			'{"format":"toolbox.manager.state","version":1,"details":{"/a/b":{"permalink":"/c/d","downloadable":true,"hasDownloadsLeft":true,"purchaseUrl":"","purchaseTitle":"","at":1}}}',
			'{"format":"toolbox.manager.state","version":1,"queue":{}}',
			'{"format":"toolbox.manager.state","version":1,"queue":[{"title":"no permalink"}]}',
			'{"format":"toolbox.manager.state","version":1,"queue":[{"permalink":"/a/b"},{"permalink":"bad"}]}',
			'{"format":"toolbox.manager.state","version":1,"queue":[{"permalink":"/a/b","details":{"unavailable":true,"at":1}}]}',
			'{"format":"toolbox.manager.state","version":1,"queue":[{"permalink":"/a/b","details":{"partial":true,"permalink":"/a/b","downloadable":false,"hasDownloadsLeft":null,"purchaseUrl":"","purchaseTitle":""}}]}',
			'{"format":"toolbox.manager.state","version":1,"queue":[{"permalink":"/a/b","details":{"permalink":"/c/d","downloadable":true,"hasDownloadsLeft":true,"purchaseUrl":"","purchaseTitle":"","at":1}}]}',
			'{"format":"toolbox.manager.state","version":1,"prefs":{"sort":"sideways"}}',
			'{"format":"toolbox.manager.state","version":1,"prefs":{"nope":true}}'
		].map((text) => {
			try {
				TbxState.parse(text);

				return 'accepted';
			} catch (error) {
				return error instanceof Error ? error.message : String(error);
			}
		});

		expect(refusals).toMatchSnapshot();
	});
});
