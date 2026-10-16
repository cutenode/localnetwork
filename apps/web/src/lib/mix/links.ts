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

/** The platform's embeddable player for a mix, for SoundCloud, Mixcloud and YouTube links. */
export function embedUrl(url: string): string | null {
	const parsed = parse(url);
	const platform = mixPlatform(url);

	if (!parsed) return null;

	if (platform === 'SoundCloud') {
		return `https://w.soundcloud.com/player/?url=${encodeURIComponent(url)}&color=%233987e5&auto_play=true&visual=false`;
	}

	if (platform === 'Mixcloud') {
		return `https://player-widget.mixcloud.com/widget/iframe/?hide_cover=1&feed=${encodeURIComponent(parsed.pathname)}`;
	}

	const youtube =
		platform === 'YouTube' &&
		(parsed.hostname === 'youtu.be' ? parsed.pathname.slice(1) : parsed.searchParams.get('v'));

	return youtube ? `https://www.youtube-nocookie.com/embed/${encodeURIComponent(youtube)}` : null;
}
