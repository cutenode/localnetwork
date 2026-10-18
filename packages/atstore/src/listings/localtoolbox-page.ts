// The SoundCloud page the localtoolbox screenshots are taken over: a playlist of made-up tracks by
// made-up artists, with the answers its content script would give about them, and the stand-in
// for chrome.* that hands those to the popup.

const TRACKS = [
	['Halflight', 'Glass Hours', 'Deep House'],
	['Neon Tide', 'Lowlands (Extended Mix)', 'Techno'],
	['Mira Okafor', 'Saltwater Static', 'Electro'],
	['Paper Moth', 'Hinterland', 'Breaks'],
	['Sundial Club', 'Second Light', 'Deep House'],
	['Velvet Arcade', 'Night Bus to Ostend', 'Minimal'],
	['Kiko Bello', 'Coral Drive', 'House'],
	['North Pier', 'Undertow (Dub)', 'Dub Techno'],
	['Glasshouse', 'Afterhours in Leith', 'Techno'],
	['Lowtide FM', 'Polaroid Summer', 'Disco']
] as const;

const slug = (text: string) =>
	text
		.toLowerCase()
		.replaceAll(/[^a-z0-9]+/g, '-')
		.replaceAll(/^-|-$/g, '');

/** Where each track can be had: SoundCloud's own download, or a gate or shop the uploader linked. */
const WHERE = [
	{ downloadable: true, purchaseUrl: '', purchaseTitle: '' },
	{ downloadable: false, purchaseUrl: 'https://hypeddit.com/neontide/lowlands', purchaseTitle: 'Free Download' },
	{ downloadable: false, purchaseUrl: 'https://miraokafor.bandcamp.com/track/saltwater-static', purchaseTitle: 'Buy' },
	{ downloadable: false, purchaseUrl: 'https://toneden.io/papermoth/post/hinterland', purchaseTitle: 'Free DL' },
	{ downloadable: true, purchaseUrl: '', purchaseTitle: '' },
	{ downloadable: false, purchaseUrl: '', purchaseTitle: '' },
	{ downloadable: false, purchaseUrl: 'https://hypeddit.com/kikobello/coraldrive', purchaseTitle: 'Free Download' },
	{ downloadable: true, purchaseUrl: '', purchaseTitle: '' },
	{
		downloadable: false,
		purchaseUrl: 'https://glasshouse.bandcamp.com/track/afterhours-in-leith',
		purchaseTitle: 'Buy'
	},
	{
		downloadable: false,
		purchaseUrl: 'https://www.droploud.com/lowtidefm/polaroid-summer',
		purchaseTitle: 'Free Download'
	}
];

const CITIES = [
	['Glasgow', 'GB'],
	['Rotterdam', 'NL'],
	['Lagos', 'NG'],
	['Bristol', 'GB'],
	['Lisbon', 'PT'],
	['Brussels', 'BE'],
	['Barcelona', 'ES'],
	['Hamburg', 'DE'],
	['Edinburgh', 'GB'],
	['Montreal', 'CA']
];

const AT = Date.parse('2026-10-01T12:00:00Z');

export const ARTWORK_HOST = 'https://i1.sndcdn.com/';

const tracks = TRACKS.map(([artist, title, genre]) => ({
	permalink: `/${slug(artist)}/${slug(title)}`,
	title,
	artist,
	artworkUrl: `${ARTWORK_HOST}artworks-${slug(title)}-t500x500.jpg`,
	genre,
	liked: null,
	reposted: null
}));

const playlist = {
	ok: true,
	url: 'https://soundcloud.com/halflight/sets/warm-up-october',
	pageType: 'playlist',
	pageKey: '/halflight/sets/warm-up-october',
	pageDetectedBy: 'dom',
	title: 'warm-up, october',
	curator: 'Halflight',
	tracks,
	skippedPlaylists: 0
};

/** The tab the popup is opened over, and what its content script answers each question with. */
export const PAGE = {
	tab: { id: 7, url: playlist.url, title: `${playlist.title} | SoundCloud`, active: true },
	answers: {
		EXTRACT_TRACKS: playlist,
		GET_DOWNLOAD_DETAILS: {
			...playlist,
			tracks: tracks.map((track, i) => ({
				...track,
				details: {
					id: 1000 + i,
					permalink: track.permalink,
					hasDownloadsLeft: true,
					genre: track.genre,
					artworkUrl: track.artworkUrl,
					at: AT,
					...WHERE[i]
				}
			})),
			lookupFailed: 0,
			resumed: false,
			unseen: false,
			error: ''
		},
		GET_ARTIST_PROFILES: {
			ok: true,
			profiles: TRACKS.map(([artist], i) => ({
				id: 2000 + i,
				permalink: `/${slug(artist)}`,
				username: artist,
				fullName: '',
				city: CITIES[i][0],
				country: CITIES[i][1],
				followers: 1200 + ((i * 7919) % 48000),
				trackCount: 12 + ((i * 31) % 90),
				verified: i % 4 === 1,
				avatarUrl: `${ARTWORK_HOST}avatars-${slug(artist)}-t200x200.jpg`,
				bio: `Bookings: bookings@${slug(artist)}.example`,
				links: [
					{ network: 'instagram', title: '', url: `https://instagram.com/${slug(artist)}` },
					{ network: 'bandcamp', title: '', url: `https://${slug(artist)}.bandcamp.com` }
				]
			})),
			failed: [],
			error: ''
		}
	}
};

/** A cover for an artwork URL: a gradient from two colours its name picks, as an SVG. */
export function cover(url: string): string {
	let hash = 2166136261;

	for (const char of url) hash = Math.imul(hash ^ char.charCodeAt(0), 16777619) >>> 0;
	const hue = hash % 360;

	return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl(${hue} 70% 58%)"/><stop offset="1" stop-color="hsl(${(hue + 70) % 360} 60% 28%)"/></linearGradient></defs><rect width="10" height="10" fill="url(#g)"/><circle cx="6.5" cy="3.5" r="1.6" fill="hsl(${(hue + 180) % 360} 80% 75% / 0.55)"/></svg>`;
}

/** What chrome.storage keeps: JSON. */
type Json = string | number | boolean | null | Json[] | { [key: string]: Json };

/** A key's change, as onChanged hands it over. */
type Change = { oldValue?: Json; newValue: Json };

type ChangeListener = (changes: Record<string, Change>, area: string) => void;

/**
 * The chrome.* the popup uses, in memory: storage areas that notify onChanged as Chrome's do,
 * the active tab, and a content script that answers from `page`. Runs in the page, so it can use
 * nothing from outside it (types aside, which are gone by then).
 */
export function installChrome(page: typeof PAGE) {
	const event = <Listener>(list: Listener[]) => ({
		addListener: (listener: Listener) => void list.push(listener),
		removeListener: (listener: Listener) => void list.splice(list.indexOf(listener) >>> 0, 1)
	});

	const changed: ChangeListener[] = [];

	const area = (name: string) => {
		const data: Record<string, Json> = {};

		return {
			/** The keys asked for that it holds, or everything for none. */
			get: async (keys?: string | string[] | null) => {
				const asked = [keys ?? Object.keys(data)].flat().filter((key) => key in data);

				return structuredClone(Object.fromEntries(asked.map((key) => [key, data[key]])));
			},
			set: async (items: Record<string, Json>) => {
				const changes = Object.fromEntries(
					Object.entries(items).map(([key, value]): [string, Change] => [key, { oldValue: data[key], newValue: value }])
				);

				Object.assign(data, structuredClone(items));

				for (const listener of changed) listener(structuredClone(changes), name);
			},
			remove: async (keys: string | string[]) => {
				for (const key of [keys].flat()) delete data[key];
			},
			setAccessLevel: async () => {}
		};
	};

	const answers = new Map(Object.entries(page.answers));

	const chrome = {
		storage: { local: area('local'), session: area('session'), onChanged: event(changed) },
		runtime: { id: 'localtoolbox', onMessage: event<() => void>([]), sendMessage: async () => undefined },
		tabs: {
			query: async () => [page.tab],
			get: async () => page.tab,
			sendMessage: async (_tab: number, message: { type: string }) => structuredClone(answers.get(message.type)),
			create: async (properties: { url: string }) => ({ id: 8, ...properties }),
			remove: async () => {}
		},
		downloads: { download: async () => 1, onChanged: event<() => void>([]) }
	};

	Object.assign(globalThis, { chrome });

	return chrome;
}
