import type { ProjectProfile } from '../src/lib/types.ts';

const at = (lat: number, lng: number) => ({ latE6: Math.round(lat * 1e6), lngE6: Math.round(lng * 1e6) });

/** Demo projects for local development. All fictional. */
export const DEMO_PROJECTS: ProjectProfile[] = [
	{
		name: 'Neon Tide',
		kind: 'solo',
		tagline: 'Rolling, hypnotic techno for late rooms',
		bio: 'Neon Tide plays long, patient techno sets that build from dub textures into peak-time pressure. Resident at a warehouse series for four years.',
		genres: ['techno', 'dub techno', 'hypnotic techno'],
		location: { city: 'Berlin', region: 'Berlin', country: 'DE', ...at(52.52, 13.405) },
		travel: { radiusKm: 600, international: true },
		formats: ['vinyl', 'cdj'],
		setTypes: ['peak', 'closing', 'allNight'],
		setLength: { minMinutes: 120, maxMinutes: 480 },
		activeSince: 2014,
		capacity: { min: 150, max: 1500 },
		fee: { tier: 'established', currency: 'EUR', minAmount: 600, maxAmount: 1500 },
		links: [{ kind: 'soundcloud', url: 'https://soundcloud.com/example-neon-tide' }],
		mixes: [{ title: 'Tidal Hours 004', url: 'https://soundcloud.com/example-neon-tide/tidal-hours-004' }],
		highlights: ['Four-year residency at Halle Nord', 'Closing set, Fusion-adjacent rave 2024']
	},
	{
		name: 'Saffron Social',
		kind: 'duo',
		tagline: 'Disco, boogie, and edits for daytime dancers',
		genres: ['disco', 'boogie', 'edits', 'nu-disco'],
		location: { city: 'Berlin', country: 'DE', ...at(52.49, 13.43) },
		travel: { radiusKm: 150 },
		formats: ['vinyl'],
		setTypes: ['daytime', 'warmup', 'b2b'],
		capacity: { min: 50, max: 600 },
		fee: { tier: 'emerging' },
		lookingFor: 'Open-air day parties, rooftop sessions, and cafe Sundays.'
	},
	{
		name: 'Lowkey Lagos',
		kind: 'collective',
		tagline: 'Amapiano and afro house crew',
		genres: ['amapiano', 'afro house', 'afrobeats', 'gqom'],
		location: { city: 'London', region: 'England', country: 'GB', ...at(51.5072, -0.1276) },
		travel: { radiusKm: 400, international: true },
		formats: ['cdj', 'controller'],
		setTypes: ['peak', 'b2b', 'allNight'],
		capacity: { min: 200, max: 2500 },
		fee: { tier: 'established', currency: 'GBP', minAmount: 500, maxAmount: 2000 },
		activeSince: 2019
	},
	{
		name: 'Pirate Signal',
		kind: 'solo',
		tagline: 'Jungle, garage, and everything in between',
		genres: ['jungle', 'drum and bass', 'uk garage', 'breaks'],
		location: { city: 'London', country: 'GB', ...at(51.54, -0.05) },
		travel: { radiusKm: 120 },
		formats: ['vinyl', 'cdj'],
		setTypes: ['peak', 'afterhours'],
		capacity: { min: 80, max: 800 },
		fee: { tier: 'emerging', currency: 'GBP', minAmount: 150, maxAmount: 400 }
	},
	{
		name: 'Velvet Static',
		kind: 'solo',
		tagline: 'Queer club classics and ballroom heat',
		genres: ['ballroom', 'house', 'jersey club', 'hyperpop'],
		location: { city: 'Brooklyn', region: 'New York', country: 'US', ...at(40.6782, -73.9442) },
		travel: { radiusKm: 300 },
		formats: ['cdj', 'controller'],
		setTypes: ['peak', 'warmup'],
		capacity: { min: 100, max: 1200 },
		fee: { tier: 'established', currency: 'USD', minAmount: 400, maxAmount: 1200 },
		pronouns: 'she/they'
	},
	{
		name: 'Sundial Soundsystem',
		kind: 'group',
		tagline: 'Balearic and downtempo for golden hours',
		genres: ['balearic', 'downtempo', 'ambient', 'disco'],
		location: { city: 'Los Angeles', region: 'California', country: 'US', ...at(34.0522, -118.2437) },
		travel: { radiusKm: 250 },
		formats: ['vinyl', 'hybrid'],
		setTypes: ['opener', 'daytime', 'warmup'],
		capacity: { min: 40, max: 500 },
		fee: { tier: 'community' }
	},
	{
		name: 'Footwork Fever',
		kind: 'solo',
		tagline: '160 bpm and climbing',
		genres: ['footwork', 'juke', 'ghetto house', 'chicago house'],
		location: { city: 'Chicago', region: 'Illinois', country: 'US', ...at(41.8781, -87.6298) },
		travel: { radiusKm: 500 },
		formats: ['cdj'],
		setTypes: ['peak', 'b2b'],
		capacity: { min: 50, max: 900 },
		fee: { tier: 'emerging', currency: 'USD', minAmount: 200, maxAmount: 600 }
	},
	{
		name: 'Moss & Marrow',
		kind: 'live',
		tagline: 'Modular live techno and electro',
		genres: ['electro', 'techno', 'experimental'],
		location: { city: 'Austin', region: 'Texas', country: 'US', ...at(30.2672, -97.7431) },
		formats: ['modular', 'live'],
		setTypes: ['liveSet', 'peak'],
		setLength: { minMinutes: 45, maxMinutes: 90 },
		capacity: { min: 50, max: 700 },
		fee: { tier: 'emerging' }
	},
	{
		name: 'Harbour Lights',
		kind: 'solo',
		tagline: 'Melodic house and progressive journeys',
		genres: ['melodic techno', 'progressive house', 'deep house'],
		location: { city: 'Melbourne', region: 'Victoria', country: 'AU', ...at(-37.8136, 144.9631) },
		travel: { radiusKm: 900, international: true },
		formats: ['cdj'],
		setTypes: ['warmup', 'peak', 'closing'],
		capacity: { min: 100, max: 3000 },
		fee: { tier: 'headliner', currency: 'AUD', minAmount: 2000, maxAmount: 6000 }
	},
	{
		name: 'Coyote Cumbia Club',
		kind: 'collective',
		tagline: 'Cumbia, dembow, and baile funk',
		genres: ['cumbia', 'dembow', 'baile funk', 'reggaeton'],
		location: { city: 'Mexico City', region: 'CDMX', country: 'MX', ...at(19.4326, -99.1332) },
		travel: { radiusKm: 300 },
		formats: ['controller', 'cdj'],
		setTypes: ['peak', 'b2b', 'allNight'],
		capacity: { min: 100, max: 2000 },
		fee: { tier: 'emerging' }
	},
	{
		name: 'Glasshouse',
		kind: 'solo',
		tagline: 'Wedding-proof open format with taste',
		genres: ['open format', 'top 40', 'r&b', 'throwbacks'],
		location: { city: 'Toronto', region: 'Ontario', country: 'CA', ...at(43.6532, -79.3832) },
		travel: { radiusKm: 200 },
		formats: ['controller'],
		setTypes: ['allNight', 'warmup'],
		capacity: { min: 50, max: 800 },
		fee: { tier: 'established', currency: 'CAD', minAmount: 800, maxAmount: 2500 }
	},
	{
		name: 'Halflight',
		kind: 'solo',
		tagline: 'Deep, dubby house for small rooms',
		genres: ['deep house', 'lo-fi house', 'dub techno'],
		location: { city: 'Potsdam', region: 'Brandenburg', country: 'DE', ...at(52.3906, 13.0645) },
		travel: { radiusKm: 80 },
		formats: ['vinyl'],
		setTypes: ['opener', 'warmup'],
		capacity: { min: 30, max: 300 },
		fee: { tier: 'community' }
	}
];

/**
 * What locallibrary would publish for neon.test, who manages Neon Tide: playing stats and three
 * sets. The tracks, artists and labels are the made-up ones from src/lib/mix/sample.ts.
 */
export const DEMO_DJ_PROFILE = {
	generatedAt: '2026-10-01T09:00:00.000Z',
	precision: 'rounded',
	basis: 'plays',
	headline: 'Hypnotic techno between 126 and 134 BPM, mixed in key',
	tags: [{ name: 'Mixes in key' }, { name: 'Long blends' }, { name: 'Builds through the night' }],
	sets: { total: { atLeast: 40 }, since: 2016, medianTracks: 22 },
	tempo: { low: 126, high: 134, median: 130 },
	arc: { builds: 70, steady: 20, windsDown: 10 },
	mixing: { harmonic: 85, smoothTempo: 90, genreChanges: 15 },
	keys: { top: [{ key: '8A' }, { key: '9A' }, { key: '7A' }, { key: '10A' }], minor: 80 },
	energy: { high: 40, mid: 45, low: 15 },
	genres: [
		{ name: 'techno', share: 55 },
		{ name: 'dub techno', share: 25 },
		{ name: 'hypnotic techno', share: 15 }
	],
	labels: [
		{ name: 'Cobble Records', share: 10 },
		{ name: 'Floor Two', share: 10 },
		{ name: 'Saltmarsh', share: 5 }
	],
	artists: [
		{ name: 'Ferrule', share: 10 },
		{ name: 'Kestrel Unit', share: 5 },
		{ name: 'Two Rooms', share: 5 }
	],
	era: { medianYear: 2021, recent: 35 },
	signature: [
		{ title: 'Night Bus', artist: 'Ferrule', bpmCentis: 12400, key: '8A' },
		{ title: 'Halogen', artist: 'Two Rooms', bpmCentis: 12600, key: '9A' },
		{ title: 'Static Bloom', artist: 'Kestrel Unit', bpmCentis: 12800, key: '3B' }
	],
	prep: { hotCues: 90, memoryCues: 60, loops: 20 },
	collection: { tracks: { atLeast: 3000 }, playlists: { count: 48 } }
};

const demoTracks = [
	{ title: 'Night Bus', artist: 'Ferrule', bpmCentis: 12400, key: '8A', startSeconds: 0 },
	{ title: 'Signal Path', artist: 'Mira Okonkwo', bpmCentis: 12500, key: '8A', startSeconds: 300 },
	{ title: 'Halogen', artist: 'Two Rooms', bpmCentis: 12600, key: '9A', startSeconds: 610 },
	{ title: 'Low Tide (Dub)', artist: 'Ferrule, Ava Lind', bpmCentis: 12400, key: '10A', startSeconds: 1020 },
	{ title: 'Static Bloom', artist: 'Kestrel Unit', bpmCentis: 12800, key: '3B', startSeconds: 1360 }
];

export const DEMO_DJ_SETS = [
	{
		title: 'Tidal Hours',
		date: '2026-09-26',
		kind: 'club',
		venue: 'Halle Nord',
		location: { $type: 'community.lexicon.location.address', name: 'Halle Nord', locality: 'Berlin', country: 'DE' },
		playedWith: [{ name: 'Halflight' }],
		tracks: demoTracks
	},
	{
		title: 'Late show',
		date: '2026-09-12',
		kind: 'radio',
		venue: 'Studio Nord',
		location: { $type: 'community.lexicon.location.address', name: 'Studio Nord', locality: 'Berlin', country: 'DE' },
		tracks: demoTracks.slice(0, 3)
	},
	{
		title: 'Tidal Hours 004',
		date: '2026-08-30',
		kind: 'mix',
		links: [{ uri: 'https://soundcloud.com/example-neon-tide', name: 'SoundCloud' }],
		tracks: demoTracks
	}
];

/**
 * Two press kits neon.test publishes for Neon Tide: one for clubs that people ask to see, and one
 * for festivals anyone signed in can open. Each is the public record and the private part.
 */
export const DEMO_EPKS = [
	{
		title: 'Clubs',
		headline: 'Rolling, hypnotic techno for late rooms',
		bio: 'Neon Tide plays long, patient techno sets that build from dub textures into peak-time pressure.',
		access: { bookedWith: 'auto', following: 'auto', promoters: 'review', anyone: 'review' },
		publicMixes: [0],
		content: {
			bio: 'Neon Tide plays long, patient techno sets that build from dub textures into peak-time pressure. Resident at Halle Nord for four years, closing sets most months, and a regular on Late Show radio.',
			mixes: [
				{ title: 'Halle Nord closing set (promo)', url: 'https://soundcloud.com/example-neon-tide/halle-nord-closing' }
			],
			quotes: [{ text: 'The best closing set we had all year.', source: 'Halle Nord' }],
			contact: { email: 'bookings@neon-tide.test', note: 'Replies within two days' },
			techRider: '2x CDJ-3000, DJM-V10, two booth monitors'
		}
	},
	{
		title: 'Festivals',
		headline: 'Big-room sets with a dub undertow',
		bio: 'Neon Tide brings the long build of a club night to a festival stage, from dub textures to peak-time techno.',
		access: { bookedWith: 'auto', following: 'auto', promoters: 'auto', anyone: 'auto' },
		publicMixes: [0],
		content: { contact: { email: 'festivals@neon-tide.test' } }
	}
];
