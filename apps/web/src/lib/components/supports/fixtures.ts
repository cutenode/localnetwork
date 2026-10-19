import type { ArtistSupports, Charts, DjSetView, SupportedArtist, SupportedTrack } from '#lib/types.ts';

/** Made-up supports for the components' stories and tests, shaped as the directory serves them. */

const FERRULE = { name: 'Ferrule', mbid: '2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41' };

const nightBus: SupportedTrack = {
	id: 'mbid:6f1c2d84-3b9e-4c1a-9d57-0e2b8a4f7c31',
	title: 'Night Bus',
	artist: 'Ferrule',
	artists: [FERRULE],
	recordingMbId: '6f1c2d84-3b9e-4c1a-9d57-0e2b8a4f7c31',
	isrc: 'DEA622600101',
	supports: 4
};

const lowTide: SupportedTrack = {
	id: 'mbid:0e7a4c19-6d3f-4b82-a1c5-93f2b8e60d17',
	title: 'Low Tide (Dub)',
	artist: 'Ferrule, Ava Lind',
	artists: [FERRULE, { name: 'Ava Lind' }],
	recordingMbId: '0e7a4c19-6d3f-4b82-a1c5-93f2b8e60d17',
	supports: 3
};

const halogen: SupportedTrack = {
	id: 'isrc:NLA112600087',
	title: 'Halogen',
	artist: 'Two Rooms',
	artists: [{ name: 'Two Rooms' }],
	isrc: 'NLA112600087',
	supports: 3
};

export const supportedArtists: SupportedArtist[] = [
	{ ...FERRULE, supports: 9, djCount: 4 },
	{ name: 'Two Rooms', supports: 3, djCount: 3 },
	{ name: 'Ava Lind', mbid: 'c13f5a88-0d2e-4b9c-a6f4-81e07b3d9c55', supports: 3, djCount: 3 }
];

export const charts: Charts = {
	tracks: [nightBus, lowTide, halogen],
	artists: supportedArtists,
	places: [
		{ city: 'Berlin', country: 'DE', count: 6 },
		{ city: 'London', country: 'GB', count: 3 }
	]
};

const set = (n: number, date: string, fields: Partial<DjSetView> = {}): DjSetView => ({
	uri: `at://did:plc:neon/computer.localnetwork.dj.set/3kset${n}`,
	cid: `bafyset${n}`,
	date,
	trackCount: 22,
	...fields
});

const halleNord = {
	venue: 'Halle Nord',
	kind: 'club',
	location: { country: 'DE', name: 'Halle Nord', locality: 'Berlin' }
};

export const artistSupports: ArtistSupports = {
	artist: { ...FERRULE, supports: 5, djCount: 3 },
	tracks: [
		{
			track: nightBus,
			plays: [
				{ did: 'did:plc:neon', set: set(1, '2026-10-13', halleNord), startSeconds: 0 },
				{ did: 'did:plc:harbour', set: set(3, '2026-10-10', { title: 'Sunday session', kind: 'club' }) },
				{
					did: 'did:plc:pirate',
					set: set(2, '2026-10-07', {
						title: 'Rooftop',
						kind: 'club',
						venue: 'Corsica Studios',
						location: { country: 'GB', locality: 'London' }
					}),
					startSeconds: 840
				},
				...Array.from({ length: 7 }, (_, i) => ({
					did: 'did:plc:neon',
					set: set(10 + i, `2026-08-${String(20 - i).padStart(2, '0')}`, { title: 'Late show', kind: 'radio' })
				}))
			]
		},
		{
			track: lowTide,
			plays: [
				{ did: 'did:plc:pirate', set: set(4, '2026-10-07', { title: 'Rooftop', kind: 'club' }), startSeconds: 1680 }
			]
		}
	],
	djs: {
		'did:plc:neon': {
			did: 'did:plc:neon',
			handle: 'neon.test',
			projects: [
				{ slug: 'neon-tide', name: 'Neon Tide' },
				{ slug: 'saffron-social', name: 'Saffron Social' }
			]
		},
		'did:plc:pirate': {
			did: 'did:plc:pirate',
			handle: 'pirate.test',
			projects: [{ slug: 'pirate-signal', name: 'Pirate Signal' }]
		},
		'did:plc:harbour': { did: 'did:plc:harbour', handle: null, projects: [] }
	}
};
