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
