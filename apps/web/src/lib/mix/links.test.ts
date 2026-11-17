import * as fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { scheme, writtenUrl } from '#lib/testing/links.ts';
import { isWebUrl } from '#lib/web-url.ts';
import { embedFrame, embedUrl, mixPlatform, plyrTrackId } from './links';

describe('mixPlatform', () => {
	it('names the platforms with players and falls back to the host', () => {
		expect(mixPlatform('https://soundcloud.com/neon/tidal-hours')).toBe('SoundCloud');
		expect(mixPlatform('https://m.mixcloud.com/neon/tidal-hours/')).toBe('Mixcloud');
		expect(mixPlatform('https://youtu.be/abc')).toBe('YouTube');
		expect(mixPlatform('https://neon.bandcamp.com/album/x')).toBe('Bandcamp');
		expect(mixPlatform('https://www.example.com/mix.mp3')).toBe('example.com');
		expect(mixPlatform('not a url')).toBeNull();
		expect(mixPlatform('javascript://soundcloud.com/%0Aalert(1)')).toBeNull();
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
			'https://player-widget.mixcloud.com/widget/iframe/?hide_cover=1&hide_artwork=1&feed=%2Fneon%2Ftidal-hours%2F'
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

describe('mix links on any address', () => {
	/** Each player's host, and the paths its players have. */
	const players = new Map([
		['w.soundcloud.com', /^\/player\/$/],
		['player-widget.mixcloud.com', /^\/widget\/iframe\/$/],
		['www.youtube-nocookie.com', /^\/embed\/[^/]+$/],
		['plyr.fm', /^\/embed\/track\/\d+$/]
	]);

	/** The hosts each named platform is on, besides their subdomains. */
	const platforms = new Map([
		['SoundCloud', ['soundcloud.com']],
		['Mixcloud', ['mixcloud.com']],
		['YouTube', ['youtube.com', 'youtu.be']],
		['Bandcamp', ['bandcamp.com']]
	]);

	it('embeds only a platform’s player, over https', () => {
		fc.assert(
			fc.property(writtenUrl, (url) => {
				const embed = embedUrl(url);

				if (embed === null) return;
				const player = new URL(embed);

				expect(player.protocol).toBe('https:');
				expect(player.username + player.password + player.port).toBe('');
				expect(players.get(player.hostname)?.test(player.pathname), embed).toBe(true);
			})
		);
	});

	it('names a platform only for its hosts, and any other web address by its host', () => {
		fc.assert(
			fc.property(writtenUrl, (url) => {
				const platform = mixPlatform(url);

				expect(platform !== null).toBe(isWebUrl(url));

				if (platform === null) return;
				const host = new URL(url).hostname.replace(/^www\./, '');
				const domains = platforms.get(platform) ?? [platform];

				expect(
					domains.some((domain) => host === domain || host.endsWith(`.${domain}`)),
					`${platform} for ${url}`
				).toBe(true);
			})
		);
	});

	// Track links on plyr.fm and hosts that only look like it, which other addresses seldom are.
	const plyrLink = fc
		.tuple(
			scheme,
			fc.constantFrom('plyr.fm', 'www.plyr.fm', 'notplyr.fm', 'plyr.fm.evil.test'),
			fc.oneof(fc.nat().map(String), fc.string()),
			fc.constantFrom('', '/')
		)
		.map(([prefix, host, id, end]) => `${prefix}://${host}/track/${id}${end}`);

	it('reads a plyr.fm track only from plyr.fm, and only as a number', () => {
		fc.assert(
			fc.property(fc.oneof(writtenUrl, plyrLink), (url) => {
				const id = plyrTrackId(url);
				const host = URL.parse(url)?.hostname.replace(/^www\./, '');

				expect(id === null || (/^\d+$/.test(id) && host === 'plyr.fm'), `${id} from ${url}`).toBe(true);
			})
		);
	});
});

describe('embedFrame', () => {
	const soundcloud = embedUrl('https://soundcloud.com/neon/tidal-hours') ?? '';
	const mixcloud = embedUrl('https://www.mixcloud.com/neon/tidal-hours/') ?? '';
	const option = (src: string, name: string) => new URL(src).searchParams.get(name);

	it("plays SoundCloud's own player on either theme, at its embed height", () => {
		expect(embedFrame(soundcloud, 'dark')).toEqual({ src: soundcloud, height: 166 });
		expect(embedFrame(soundcloud, 'light')).toEqual({ src: soundcloud, height: 166 });
	});

	it("plays Mixcloud's dark widget, or its light one on the light theme", () => {
		const dark = embedFrame(mixcloud, 'dark');
		const light = embedFrame(mixcloud, 'light');

		expect([dark.height, light.height]).toEqual([120, 120]);
		expect(dark.src).toBe(mixcloud);
		expect(option(light.src, 'light')).toBe('1');
		expect(option(light.src, 'feed')).toBe('/neon/tidal-hours/');
	});

	it('leaves the other players as they are', () => {
		const youtube = embedUrl('https://youtu.be/abc123') ?? '';

		expect(embedFrame(youtube, 'light')).toEqual({ src: youtube, height: 160 });
		expect(embedFrame('about:blank', 'dark')).toEqual({ src: 'about:blank', height: 160 });
	});
});
