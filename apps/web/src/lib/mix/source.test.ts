import { describe, expect, it } from 'vitest';
import { linkSources, playable, preferred, type PlayerSource, trackAt } from './source';

const ARCHIVE = 'https://archive.org/download/neon-tidal-hours/tidal-hours.mp3';

describe('linkSources', () => {
	it('plays an archive.org audio download as it is, linking its item page', () => {
		expect(linkSources([{ uri: ARCHIVE }])).toEqual([
			{ kind: 'audio', src: ARCHIVE, from: 'archive.org', link: 'https://archive.org/details/neon-tidal-hours' }
		]);
		expect(linkSources([{ uri: 'https://archive.org/download/item/disc%201/Side%20A.FLAC' }])[0]).toMatchObject({
			kind: 'audio',
			from: 'archive.org'
		});
	});

	it("doesn't play archive.org pages, other files, plain http or look-alike hosts", () => {
		expect(
			linkSources([
				{ uri: 'https://archive.org/details/neon-tidal-hours' },
				{ uri: 'https://archive.org/download/neon-tidal-hours/cover.jpg' },
				{ uri: 'https://archive.org/download/neon-tidal-hours.mp3' },
				{ uri: 'http://archive.org/download/neon-tidal-hours/tidal-hours.mp3' },
				{ uri: 'https://notarchive.org/download/neon-tidal-hours/tidal-hours.mp3' },
				{ uri: 'https://archive.org.example/download/neon-tidal-hours/tidal-hours.mp3' },
				{ uri: 'not a url' }
			])
		).toEqual([]);
	});

	it("uses the platforms' players for the links they have, in order", () => {
		const sources = linkSources([
			{ uri: 'https://www.mixcloud.com/neon/tidal-hours/' },
			{ uri: 'https://neon.example/tidal-hours' },
			{ uri: 'https://soundcloud.com/neon/tidal-hours' }
		]);

		expect(sources.map((source) => source.kind === 'embed' && source.platform)).toEqual(['Mixcloud', 'SoundCloud']);
		expect(sources[0]).toMatchObject({ link: 'https://www.mixcloud.com/neon/tidal-hours/' });
		expect(linkSources(undefined)).toEqual([]);
	});
});

describe('preferred', () => {
	const embed: PlayerSource = {
		kind: 'embed',
		src: 'https://w.soundcloud.com/player/',
		platform: 'SoundCloud',
		link: 'x'
	};

	const archive: PlayerSource = { kind: 'audio', src: ARCHIVE, from: 'archive.org' };
	const pds: PlayerSource = { kind: 'audio', src: '/media/audio/did:plc:dj/bafy', from: 'pds' };

	it('takes audio over a player, then the order given', () => {
		expect(preferred([embed, null, archive, pds])).toBe(archive);
		expect(preferred([null, pds, archive])).toBe(pds);
		expect(preferred([null, embed, { ...embed, platform: 'Mixcloud' }])).toBe(embed);
		expect(preferred([null])).toBeNull();
	});
});

describe('playable', () => {
	it('knows from the record whether there may be something to play', () => {
		expect(playable({ recording: { uri: 'at://did:plc:dj/fm.plyr.track/3abc' } })).toBe(true);
		expect(playable({ recording: { uri: 'at://did:plc:dj/computer.localnetwork.dj.recording/3abc' } })).toBe(true);
		expect(playable({ links: [{ uri: ARCHIVE }] })).toBe(true);
		expect(playable({ links: [{ uri: 'https://youtu.be/abc' }] })).toBe(true);
		expect(playable({ recording: { uri: 'at://did:plc:dj/app.bsky.feed.post/3abc' } })).toBe(false);
		expect(playable({ links: [{ uri: 'https://neon.example/mix' }] })).toBe(false);
		expect(playable({})).toBe(false);
	});
});

describe('trackAt', () => {
	const tracks = [
		{ title: 'Opener', startSeconds: 0 },
		{ title: 'Night Bus', startSeconds: 300 },
		{ title: 'No time' },
		{ title: 'Closer', startSeconds: 600 }
	];

	it('finds the last track started by then', () => {
		expect(trackAt(tracks, 0)).toBe(0);
		expect(trackAt(tracks, 299.9)).toBe(0);
		expect(trackAt(tracks, 300)).toBe(1);
		expect(trackAt(tracks, 599)).toBe(1);
		expect(trackAt(tracks, 3600)).toBe(3);
	});

	it('is -1 before the first start, and with no starts at all', () => {
		expect(trackAt([{ title: 'Late', startSeconds: 30 }], 10)).toBe(-1);
		expect(trackAt([{ title: 'No time' }], 100)).toBe(-1);
		expect(trackAt([], 100)).toBe(-1);
	});

	it('follows the starts, not the order, when the list is out of order', () => {
		const unsorted = [
			{ title: 'B', startSeconds: 300 },
			{ title: 'A', startSeconds: 0 },
			{ title: 'C', startSeconds: 600 }
		];

		expect(trackAt(unsorted, 100)).toBe(1);
		expect(trackAt(unsorted, 400)).toBe(0);
		expect(trackAt(unsorted, 700)).toBe(2);
	});
});
