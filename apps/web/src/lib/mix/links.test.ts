import { describe, expect, it } from 'vitest';
import { embedUrl, mixPlatform } from './links';

describe('mixPlatform', () => {
	it('names the platforms with players and falls back to the host', () => {
		expect(mixPlatform('https://soundcloud.com/neon/tidal-hours')).toBe('SoundCloud');
		expect(mixPlatform('https://m.mixcloud.com/neon/tidal-hours/')).toBe('Mixcloud');
		expect(mixPlatform('https://youtu.be/abc')).toBe('YouTube');
		expect(mixPlatform('https://neon.bandcamp.com/album/x')).toBe('Bandcamp');
		expect(mixPlatform('https://www.example.com/mix.mp3')).toBe('example.com');
		expect(mixPlatform('not a url')).toBeNull();
	});

	it("doesn't take a look-alike host for a platform", () => {
		expect(mixPlatform('https://notsoundcloud.com/x')).toBe('notsoundcloud.com');
	});
});

describe('embedUrl', () => {
	it('builds each platform’s player', () => {
		expect(embedUrl('https://soundcloud.com/neon/tidal-hours')).toContain(
			'w.soundcloud.com/player/?url=https%3A%2F%2Fsoundcloud.com%2Fneon%2Ftidal-hours'
		);
		expect(embedUrl('https://www.mixcloud.com/neon/tidal-hours/')).toBe(
			'https://player-widget.mixcloud.com/widget/iframe/?hide_cover=1&feed=%2Fneon%2Ftidal-hours%2F'
		);
		expect(embedUrl('https://www.youtube.com/watch?v=abc123')).toBe('https://www.youtube-nocookie.com/embed/abc123');
		expect(embedUrl('https://youtu.be/abc123')).toBe('https://www.youtube-nocookie.com/embed/abc123');
		expect(embedUrl('https://plyr.fm/track/1567')).toBe('https://plyr.fm/embed/track/1567');
		expect(embedUrl('https://www.plyr.fm/track/1567/')).toBe('https://plyr.fm/embed/track/1567');
	});

	it('has no player for other sites or bad links', () => {
		expect(embedUrl('https://example.com/mix.mp3')).toBeNull();
		expect(embedUrl('nope')).toBeNull();
		expect(embedUrl('https://notsoundcloud.com/neon/tidal-hours')).toBeNull();
		expect(embedUrl('https://www.youtube.com/@neon')).toBeNull();
		expect(embedUrl('https://plyr.fm/u/neon')).toBeNull();
		expect(embedUrl('https://plyr.fm/track/1567/comments')).toBeNull();
		expect(embedUrl('https://notplyr.fm/track/1567')).toBeNull();
	});
});
