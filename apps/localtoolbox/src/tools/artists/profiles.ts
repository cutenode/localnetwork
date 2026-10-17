// Who is behind a track, and where else to find them: the pure logic under the
// artists panel.
//
// An artist here is the uploader of a track — the account the track's
// permalink names, which on a playlist page is also the name the row shows.
// That path is the artist's identity throughout this tool: what SoundCloud
// resolves to a profile, what the list holds an artist once under, and what
// takes one off again. A profile is what SoundCloud said about the account
// (content.ts, GET_ARTIST_PROFILES), and its socials are the links the
// uploader put on it — plus whatever addresses they wrote into the bio, which
// is where a good many of them put the links that matter.

import { storedList } from '$lib/stored';
import type { Artist, ArtistEntry, KnownProfile, ProfileEntry, Track } from '$lib/types';
import { fallback, z } from '$lib/zod';

/** An artist with a profile to show. */
export type ProfiledEntry = ArtistEntry & { profile: KnownProfile };

/** A network's badge. */
export interface Network {
	name: string;
	/** The monogram. */
	badge: string;
	/** The hosts it is on, and under; none for the two told apart otherwise. */
	hosts: string[];
	background: string;
	/** The monogram's, where white would not read. */
	color?: string;
}

/** One link as a badge and an opened row draw it (linkOf). */
export interface Link {
	/** The address as one link is told from another (keyOf). */
	key: string;
	/** Its id in NETWORKS. */
	network: string;
	/** The network's name, or a website's host. */
	name: string;
	badge: string;
	/** The badge's colours, as a style attribute. */
	style: string;
	url: string;
	/** The uploader's own title for it, or ''. */
	title: string;
	source: 'profile' | 'bio';
}

/** One line of an opened row (rowFacts). */
export interface Fact {
	id: string;
	label: string;
	/** What the line shows: 'unknown' where nothing is known. */
	value: string;
	/** What a click copies, or '' for nothing. */
	copy: string;
	/** The address its arrow opens, or '' for no arrow. */
	open: string;
	/** A link's own title. */
	hint?: string;
}

/**
 * The first segment of a track's permalink path: the uploader's own path.
 * @param permalink what is not a track's path, or is missing, is ''
 */
export const artistPathOf = (permalink?: string | null) => {
	const match = /^\/([^/?#\s]+)\/[^/?#\s]+$/.exec(permalink ?? '');

	return match ? `/${match[1]}` : '';
};

/** Whether a string is an artist's path: a slash, one segment, nothing else. */
export const isArtistPath = (value?: string | null) => /^\/[^/?#\s]+$/.test(value ?? '');

/** The profile page an artist's path opens. */
export const profileUrl = (permalink: string) => `https://soundcloud.com${permalink}`;

const words = (value?: string) => (value ?? '').trim();

/**
 * The artists behind a list of tracks, one per uploader, in the order first
 * seen: each with the name the page gave and the tracks that brought it in.
 *
 * A track with no permalink names nobody and is skipped; two tracks by one
 * uploader are one artist with two tracks. The name is whatever the row
 * called the artist, and '' when no row named one (nameOf falls back to the
 * path's own segment) — a badge tile with no artist on it still has an uploader.
 *
 * @param tracks a page's, the Manager's list's or the queue's
 */
export function artistsOf(tracks: Track[]) {
	const byPath: Map<string, Artist> = new Map();

	for (const track of Array.isArray(tracks) ? tracks : []) {
		const permalink = artistPathOf(track?.permalink);

		if (!permalink) continue;
		const seen = { permalink: track.permalink, title: words(track.title) };
		const name = words(track.artist);
		const held = byPath.get(permalink);

		if (held) {
			if (!held.name) held.name = name;

			if (!held.tracks.some((one) => one.permalink === seen.permalink)) held.tracks.push(seen);
			continue;
		}

		byPath.set(permalink, { permalink, name, tracks: [seen] });
	}

	return [...byPath.values()];
}

/** What to call an artist: the page's name for it, else SoundCloud's, else its path. */
export const nameOf = (entry: ArtistEntry) => entry.name || entry.profile?.username || entry.permalink.slice(1);

// --- the networks ------------------------------------------------------------
//
// A badge per network: a lettered monogram in the site's colour rather than
// its logo, so the panel ships no third-party artwork (the search badges do
// the same). Which network a link is on is read off its address rather than
// trusted from SoundCloud's own `network` field, so a link written into a
// bio is filed exactly as one put on the profile would be — and the field is
// the fallback for an address on a host this list does not know.

/** By the id a link is filed under. */
export const NETWORKS = {
	instagram: { name: 'Instagram', badge: 'IG', hosts: ['instagram.com'], background: '#e1306c' },
	twitter: { name: 'X', badge: 'X', hosts: ['twitter.com', 'x.com'], background: '#000000' },
	facebook: {
		name: 'Facebook',
		badge: 'fb',
		hosts: ['facebook.com', 'fb.com', 'fb.me'],
		background: '#1877f2'
	},
	youtube: { name: 'YouTube', badge: 'YT', hosts: ['youtube.com', 'youtu.be'], background: '#ff0000' },
	tiktok: { name: 'TikTok', badge: 'TT', hosts: ['tiktok.com'], background: '#010101' },
	threads: { name: 'Threads', badge: 'TH', hosts: ['threads.net', 'threads.com'], background: '#000000' },
	spotify: {
		name: 'Spotify',
		badge: 'SP',
		hosts: ['spotify.com'],
		background: '#1db954',
		color: '#000000'
	},
	apple: {
		name: 'Apple Music',
		badge: 'AM',
		hosts: ['music.apple.com', 'itunes.apple.com'],
		background: '#fa243c'
	},
	bandcamp: { name: 'Bandcamp', badge: 'bc', hosts: ['bandcamp.com'], background: '#1da0c3' },
	beatport: {
		name: 'Beatport',
		badge: 'BP',
		hosts: ['beatport.com'],
		background: '#1e1e1e',
		color: '#01ff95'
	},
	mixcloud: { name: 'Mixcloud', badge: 'MC', hosts: ['mixcloud.com'], background: '#5000ff' },
	residentadvisor: {
		name: 'Resident Advisor',
		badge: 'RA',
		hosts: ['ra.co', 'residentadvisor.net'],
		background: '#0f0f0f'
	},
	discogs: { name: 'Discogs', badge: 'DG', hosts: ['discogs.com'], background: '#333333' },
	twitch: { name: 'Twitch', badge: 'TW', hosts: ['twitch.tv'], background: '#9146ff' },
	discord: { name: 'Discord', badge: 'DC', hosts: ['discord.gg', 'discord.com'], background: '#5865f2' },
	telegram: { name: 'Telegram', badge: 'TG', hosts: ['t.me', 'telegram.me'], background: '#26a5e4' },
	snapchat: {
		name: 'Snapchat',
		badge: 'SN',
		hosts: ['snapchat.com'],
		background: '#fffc00',
		color: '#000000'
	},
	patreon: { name: 'Patreon', badge: 'PT', hosts: ['patreon.com'], background: '#ff424d' },
	linktree: {
		name: 'Linktree',
		badge: 'LT',
		hosts: ['linktr.ee'],
		background: '#43e660',
		color: '#000000'
	},
	soundcloud: { name: 'SoundCloud', badge: 'sc', hosts: ['soundcloud.com'], background: '#ff5500' },
	email: { name: 'email', badge: '@', hosts: [], background: '#52525b' },
	personal: { name: 'website', badge: 'www', hosts: [], background: '#52525b' }
} satisfies Record<string, Network>;

type NetworkId = keyof typeof NETWORKS;

/** Own keys only: `'constructor' in NETWORKS` is true too. */
const isNetworkId = (id: string): id is NetworkId => Object.hasOwn(NETWORKS, id);

const NETWORK_IDS = Object.keys(NETWORKS).filter(isNetworkId);

/** The host of an address, lower-cased and without a leading www. */
const hostOf = (url: string) => {
	try {
		return new URL(url).hostname.toLowerCase().replace(/^www\./, '');
	} catch (_e) {
		return '';
	}
};

/**
 * Which network an address is on: the one whose host it is under, else the
 * one SoundCloud filed it under when that is a network this knows, else a
 * website. A `mailto:` is email, whatever it was filed under.
 *
 * @param filed SoundCloud's `network` for it
 * @returns an id in NETWORKS
 */
export function networkOf(url: string, filed = ''): NetworkId {
	if (/^mailto:/i.test(url)) return 'email';
	const host = hostOf(url);

	for (const id of NETWORK_IDS) {
		if (NETWORKS[id].hosts.some((one) => host === one || host.endsWith(`.${one}`))) return id;
	}

	const given = words(filed).toLowerCase();

	return isNetworkId(given) && given !== 'email' ? given : 'personal';
}

/** The badge's colours, as a style attribute. */
const styleOf = (network: Network) => `background:${network.background};color:${network.color || '#ffffff'}`;

// An address as one link is told from another: scheme and www dropped, the
// case folded, a trailing slash gone — so the profile's Instagram and the bio's
// are one link rather than two badges opening the same page.
const keyOf = (url: string) =>
	url
		.trim()
		.toLowerCase()
		.replace(/^https?:\/\//, '')
		.replace(/^www\./, '')
		.replace(/^mailto:/, '')
		.replace(/\/+$/, '');

/** @param filed SoundCloud's `network` for it */
const linkOf = (url: string, filed: string | undefined, title: string | undefined, source: 'profile' | 'bio'): Link => {
	const id = networkOf(url, filed);
	const network = NETWORKS[id];
	const host = hostOf(url);

	return {
		key: keyOf(url),
		network: id,
		// A website is named by its host, since "website" says nothing about
		// which; every other network is named for itself.
		name: id === 'personal' && host ? host : network.name,
		badge: network.badge,
		style: styleOf(network),
		url: url.trim(),
		title: words(title),
		/** 'profile' for a link SoundCloud lists, 'bio' for one read out of the text. */
		source
	};
};

// What an address in running text looks like: something with a scheme, or a
// www, or a host this tool knows followed by a path. A bare "bicep.com" with
// none of those is left alone — "e.g." and "vs." are not links, and a word
// that merely contains a dot is far likelier than a domain typed without
// its scheme.
const KNOWN_HOSTS = Object.values(NETWORKS)
	.flatMap((network) => network.hosts)
	.map((host) => RegExp.escape(host))
	.join('|');

const BIO_URL = new RegExp(
	`\\b(?:https?://[^\\s<>"'()]+|www\\.[^\\s<>"'()]+|(?:[\\w-]+\\.)*(?:${KNOWN_HOSTS})/[^\\s<>"'()]*)`,
	'gi'
);

const BIO_EMAIL = /\b[\w.+-]+@[\w-]+(?:\.[\w-]+)+\b/g;

// Punctuation a sentence ends an address with, which is not part of it.
const trimmed = (found: string) => found.replace(/[.,;:!?)\]]+$/, '');

/**
 * The addresses written into a bio, as links: full ones, `www.` ones, and
 * known hosts with a path; and email addresses. The addresses first, then the
 * emails, each in the order they appear.
 */
export function bioLinks(bio?: string) {
	const text = words(bio);

	if (!text) return [];
	const links: Link[] = [];

	for (const match of text.matchAll(BIO_URL)) {
		const found = trimmed(match[0]);
		const url = /^https?:\/\//i.test(found) ? found : `https://${found}`;
		links.push(linkOf(url, '', '', 'bio'));
	}

	for (const match of text.matchAll(BIO_EMAIL)) {
		links.push(linkOf(`mailto:${match[0]}`, 'email', '', 'bio'));
	}

	return links;
}

/**
 * The links a stored profile lists that have an address, which is all a link
 * cannot go without; a profile restored from an older file can hold anything.
 */
const profileLinksOf = storedList(
	z.object({
		url: z.string().check(z.refine((url) => url.trim() !== '')),
		network: fallback(z.string(), ''),
		title: fallback(z.string(), '')
	})
);

/**
 * Every link an artist offers, one per address: the ones SoundCloud lists on
 * the profile first, in its order, then the ones read out of the bio that
 * none of those already covered.
 */
export function linksOf(profile: ProfileEntry | null) {
	if (!profile || profile.unavailable) return [];
	const links: Link[] = [];
	const seen = new Set();

	const take = (link: Link) => {
		if (seen.has(link.key)) return;
		seen.add(link.key);
		links.push(link);
	};

	for (const one of profileLinksOf(profile.links)) take(linkOf(one.url, one.network, one.title, 'profile'));

	for (const link of bioLinks(profile.bio)) take(link);

	return links;
}

// --- what a row says ---------------------------------------------------------

/** Whether an artist has been asked about and SoundCloud had no profile. */
export const isUnavailable = (entry: ArtistEntry) => entry.profile?.unavailable === true;

/** Whether nobody has asked SoundCloud about an artist yet. */
export const isUnchecked = (entry: ArtistEntry) => !entry.profile;

/** Whether an artist has a profile to show. */
export const hasProfile = (entry: ArtistEntry): entry is ProfiledEntry => !!entry.profile && !entry.profile.unavailable;

/**
 * "1.2K", "34M" — a follower count at the width a row has for it.
 * @param n anything but a whole number is ''
 */
export function shortCount(n?: number | null) {
	// Number.isInteger has ruled out null and undefined for everything after it.
	if (!Number.isInteger(n) || n! < 0) return '';

	if (n! < 1000) return String(n);
	const scaled: [number, string] = n! < 1000000 ? [n! / 1000, 'K'] : [n! / 1000000, 'M'];
	const [value, unit] = scaled;

	return `${value < 10 ? value.toFixed(1).replace(/\.0$/, '') : Math.round(value)}${unit}`;
}

/** Where an artist says they are: "Belfast, GB", or either half alone. */
export const placeOf = (profile?: ProfileEntry | null) => [profile?.city, profile?.country].filter(Boolean).join(', ');

/**
 * The line under an artist's name. What is known — their place, their
 * followers, how many links — or, when nothing is, why: nobody has asked, or
 * SoundCloud had no profile for the path.
 *
 * @param options `checking` while a lookup is asking about it
 */
export function profileLine(entry: ArtistEntry, { checking = false } = {}) {
	if (checking) return 'looking up…';

	if (isUnchecked(entry)) return 'not looked up';

	if (!hasProfile(entry)) return 'no public profile';
	const { profile } = entry;
	const links = linksOf(profile).length;

	return [
		placeOf(profile),
		profile.followers == null ? '' : `${shortCount(profile.followers)} followers`,
		links === 0 ? 'no links' : `${links} link${links === 1 ? '' : 's'}`
	]
		.filter(Boolean)
		.join(' · ');
}

/** Everything about an artist the search box should find. */
export function searchText(entry: ArtistEntry) {
	const parts: (string | undefined)[] = [entry.name, entry.permalink, entry.from];

	if (hasProfile(entry)) {
		const { profile } = entry;
		parts.push(profile.username, profile.fullName, profile.city, profile.country, profile.bio);

		for (const link of linksOf(profile)) parts.push(link.name, link.url, link.title);
	}

	for (const track of entry.tracks) parts.push(track.title);

	return parts.filter(Boolean).join(' ');
}

/**
 * What a row opens into: every field of the artist, one a line — the profile
 * page and every link first, each with an arrow to open it, then what
 * SoundCloud said and what the list knows. `copy` is what a click puts on
 * the clipboard, and `''` for a field nothing is known about: copying the
 * word "unknown" into a spreadsheet would put a claim there nobody made.
 */
export function rowFacts(entry: ArtistEntry) {
	const facts: Fact[] = [];
	const address = profileUrl(entry.permalink);
	facts.push({ id: 'soundcloud', label: 'SoundCloud', value: address, copy: address, open: address });

	for (const link of linksOf(entry.profile)) {
		facts.push({
			id: `link:${link.key}`,
			label: link.name,
			value: link.url,
			copy: link.url,
			open: link.url,
			hint: link.title
		});
	}

	/** @param value '' or absent where nothing is known */
	const fact = (id: string, label: string, value?: string) =>
		facts.push({ id, label, value: value || 'unknown', copy: value || '', open: '' });

	if (hasProfile(entry)) {
		const { profile } = entry;

		// SoundCloud's own name for the account, where the page called it
		// something else (a Likes row shows the display artist, not the uploader).
		if (profile.username && profile.username !== entry.name) {
			fact('username', 'name', profile.username);
		}

		fact('place', 'place', placeOf(profile));
		fact('followers', 'followers', profile.followers == null ? '' : profile.followers.toLocaleString('en-US'));
		fact('bio', 'bio', profile.bio);
	}

	fact('tracks', 'tracks', entry.tracks.map((one) => one.title || one.permalink).join(', '));
	fact('from', 'from', entry.from);

	return facts;
}

/** The sort orders the panel offers. */
export const SORTS = [
	{ value: 'name', label: 'by name' },
	{ value: 'list', label: 'list order' },
	{ value: 'followers', label: 'most followed' }
];

export const SORT_VALUES = SORTS.map((sort) => sort.value);

const followersOf = (entry: ArtistEntry) => (hasProfile(entry) ? (entry.profile.followers ?? -1) : -1);

/**
 * The entries in one of those orders. `list` is the order they came in.
 * @param order a value of SORTS; any other keeps the list's order
 */
export function sorted(entries: ArtistEntry[], order: string) {
	const copy = [...entries];

	if (order === 'name') {
		return copy.sort((a, b) => nameOf(a).localeCompare(nameOf(b), undefined, { sensitivity: 'base' }));
	}

	if (order === 'followers') {
		return copy.sort((a, b) => followersOf(b) - followersOf(a));
	}

	return copy;
}

/**
 * The list as text to paste: one block per artist — the name, the profile,
 * then every link on its own line — with a blank line between them.
 */
export function asText(entries: ArtistEntry[]) {
	return entries
		.map((entry) => {
			const lines = [nameOf(entry), profileUrl(entry.permalink)];

			for (const link of linksOf(entry.profile)) lines.push(`${link.name}: ${link.url}`);

			return lines.join('\n');
		})
		.join('\n\n');
}

/**
 * How many badges a row shows before it says "+n" for the rest. Three: a row
 * is 400px wide, and the name has to survive beside them — the rest are a
 * chevron away, in the opened row.
 */
export const ROW_BADGES = 3;
