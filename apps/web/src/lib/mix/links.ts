/** Where a mix link plays. */
const platforms: [host: string, name: string][] = [
	['soundcloud.com', 'SoundCloud'],
	['mixcloud.com', 'Mixcloud'],
	['youtube.com', 'YouTube'],
	['youtu.be', 'YouTube'],
	['bandcamp.com', 'Bandcamp']
];

function parse(url: string): URL | null {
	try {
		return new URL(url);
	} catch {
		return null;
	}
}

/** The platform a mix is on, by name, or its host for any other site. */
export function mixPlatform(url: string): string | null {
	const host = parse(url)?.hostname.replace(/^www\./, '');

	if (!host) return null;

	return platforms.find(([domain]) => host === domain || host.endsWith(`.${domain}`))?.[1] ?? host;
}

/** The track a plyr.fm track link (`https://plyr.fm/track/<id>`) is to, by plyr.fm's ID, or null for any other link. */
export function plyrTrackId(url: string): string | null {
	const parsed = parse(url);

	if (!parsed || mixPlatform(url) !== 'plyr.fm') return null;

	return /^\/track\/(\d+)\/?$/.exec(parsed.pathname)?.[1] ?? null;
}

/** The platform's embeddable player for a mix, for SoundCloud, Mixcloud, YouTube and plyr.fm links. */
export function embedUrl(url: string): string | null {
	const parsed = parse(url);
	const platform = mixPlatform(url);

	if (!parsed) return null;

	if (platform === 'SoundCloud') {
		return `https://w.soundcloud.com/player/?url=${encodeURIComponent(url)}&color=%233987e5&auto_play=true&visual=false`;
	}

	if (platform === 'Mixcloud') {
		return `https://player-widget.mixcloud.com/widget/iframe/?hide_cover=1&hide_artwork=1&feed=${encodeURIComponent(parsed.pathname)}`;
	}

	const plyr = plyrTrackId(url);

	if (plyr) return `https://plyr.fm/embed/track/${plyr}`;

	const youtube =
		platform === 'YouTube' &&
		(parsed.hostname === 'youtu.be' ? parsed.pathname.slice(1) : parsed.searchParams.get('v'));

	return youtube ? `https://www.youtube-nocookie.com/embed/${encodeURIComponent(youtube)}` : null;
}

/** The site's palettes: dark, and light under a `light` class on the root (packages/tokens/theme.css). */
type Theme = 'dark' | 'light';

/**
 * A player from `embedUrl` in the site's theme, and the height in pixels it lays out for. Mixcloud's widget is dark
 * unless asked for its light one. The others have no theme option: SoundCloud's player is white on either theme, at the
 * 166px of SoundCloud's own embed code.
 */
export function embedFrame(src: string, theme: Theme) {
	const url = parse(src);

	if (url?.hostname === 'player-widget.mixcloud.com') {
		if (theme === 'light') url.searchParams.set('light', '1');

		return { src: url.href, height: 120 };
	}

	return { src, height: url?.hostname === 'w.soundcloud.com' ? 166 : 160 };
}
