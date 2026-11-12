import { describe, expect, it } from 'vitest';
import type { DjSet } from '#lib/dj.ts';
import type { PageMeta } from '#lib/meta.ts';
import type { MixReport } from '#lib/mix/report.ts';
import type { ProfileView, ProjectView } from '#lib/types.ts';
import { artistMeta, mixMeta, performerMeta, pressKitMeta, profileMeta, setMeta } from './meta';
import {
	mixRecording,
	musicBrainzArtist,
	performerProfile,
	personProfile,
	jsonLdFor,
	pressKitProfile,
	setPlaylist
} from './structured-data';

const SITE = 'https://localnetwork.test';

const project: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/1',
	rkey: '1',
	cid: 'c',
	slug: 'neon-tide',
	profile: {
		name: 'Neon Tide',
		kind: 'solo',
		tagline: 'Rolling techno',
		genres: ['techno', 'electro'],
		location: { city: 'Berlin', region: 'BE', country: 'DE' },
		links: [
			{ kind: 'website', url: 'https://neontide.test' },
			{ kind: 'soundcloud', url: 'http://soundcloud.test/neon' },
			{ kind: 'other', url: 'javascript:alert(1)' },
			{ kind: 'other', url: 'not a link' }
		]
	},
	managers: [],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:a',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-02-01T00:00:00Z',
	avatarUrl: '/media/did:plc:root/bafyavatar'
};

/** The same performer with only what a project must have. */
const bare: ProjectView = {
	...project,
	avatarUrl: null,
	profile: { name: 'Neon Tide', kind: 'solo', genres: [], location: { city: 'Berlin', country: 'DE' } }
};

const neonTide = {
	'@type': 'MusicGroup',
	name: 'Neon Tide',
	url: 'https://localnetwork.test/projects/neon-tide',
	image: 'https://localnetwork.test/media/did:plc:root/bafyavatar',
	description: 'Rolling techno',
	genre: ['techno', 'electro'],
	location: {
		'@type': 'Place',
		address: { '@type': 'PostalAddress', addressLocality: 'Berlin', addressRegion: 'BE', addressCountry: 'DE' }
	},
	sameAs: ['https://neontide.test', 'http://soundcloud.test/neon']
};

const bareNeonTide = {
	'@type': 'MusicGroup',
	name: 'Neon Tide',
	url: 'https://localnetwork.test/projects/neon-tide',
	location: { '@type': 'Place', address: { '@type': 'PostalAddress', addressLocality: 'Berlin', addressCountry: 'DE' } }
};

/**
 * A page's metadata with a link preview card for its picture, as a page without one of its own gets. Structured data
 * names the performer's or person's own picture, so none of this card may reach it.
 */
const carded = (meta: PageMeta): PageMeta => ({
	...meta,
	image: { url: 'https://localnetwork.test/og/card.png', alt: 'Local Network' }
});

const report = (mix: Partial<MixReport['mix']>): Pick<MixReport, 'mix'> => ({
	mix: { title: 'Sunday Warm-up', durationSecs: 7092, ...mix }
});

describe('jsonLdFor', () => {
	it('adds the schema.org context to what the page is about', () => {
		expect(jsonLdFor(performerMeta(SITE, project), () => ({ '@type': 'Thing', name: 'x' }))).toEqual({
			'@context': 'https://schema.org',
			'@type': 'Thing',
			name: 'x'
		});
	});
});

describe('performers', () => {
	it('makes a performer page a profile page about a music group, with absolute addresses and web links only', () => {
		expect(jsonLdFor(carded(performerMeta(SITE, project)), performerProfile(project))).toEqual({
			'@context': 'https://schema.org',
			'@type': 'ProfilePage',
			url: 'https://localnetwork.test/projects/neon-tide',
			dateCreated: '2026-01-01T00:00:00Z',
			dateModified: '2026-02-01T00:00:00Z',
			mainEntity: neonTide
		});
	});

	it('leaves out what a performer has not filled in', () => {
		expect(jsonLdFor(carded(performerMeta(SITE, bare)), performerProfile(bare))?.mainEntity).toEqual(bareNeonTide);
	});

	it("names a press kit's profile page for the kit, from its public part", () => {
		const epk = { rkey: 'k1', cid: 'bafykit', record: { title: 'Summer 2026', headline: 'Out now' } };

		expect(jsonLdFor(carded(pressKitMeta(SITE, project, epk)), pressKitProfile(project, epk.record))).toEqual({
			'@context': 'https://schema.org',
			'@type': 'ProfilePage',
			name: 'Summer 2026',
			url: 'https://localnetwork.test/projects/neon-tide/press-kits/k1',
			description: 'Out now',
			mainEntity: neonTide
		});
		expect(
			jsonLdFor(carded(pressKitMeta(SITE, bare, epk)), pressKitProfile(bare, { title: 'Summer 2026' }))
		).not.toHaveProperty('description');
	});
});

describe('mixes', () => {
	const recording = (performer: ProjectView, mix: Partial<MixReport['mix']>) =>
		jsonLdFor(
			carded(mixMeta(SITE, performer, { title: 'Warm-up' }, 2, 12)),
			mixRecording(performer, { title: 'Warm-up' }, report(mix))
		);

	it('makes a mix a recording by its performer, with an ISO 8601 duration', () => {
		expect(recording(project, { date: '2026-10-04' })).toEqual({
			'@context': 'https://schema.org',
			'@type': 'MusicRecording',
			name: 'Warm-up',
			url: 'https://localnetwork.test/projects/neon-tide/mixes/2',
			byArtist: { '@type': 'MusicGroup', name: 'Neon Tide', url: 'https://localnetwork.test/projects/neon-tide' },
			duration: 'PT1H58M12S',
			image: 'https://localnetwork.test/media/did:plc:root/bafyavatar',
			dateCreated: '2026-10-04'
		});
	});

	it('writes only the units a duration has, and none under a second', () => {
		expect(recording(project, { durationSecs: 3600 })?.duration).toBe('PT1H');
		expect(recording(project, { durationSecs: 1739.6 })?.duration).toBe('PT29M');
		expect(recording(project, { durationSecs: 45 })?.duration).toBe('PT45S');
		expect(recording(project, { durationSecs: 3605 })?.duration).toBe('PT1H5S');
		expect(recording(project, { durationSecs: 0.4 })).not.toHaveProperty('duration');
	});

	it("leaves out a date that isn't a calendar date, and a picture the performer hasn't got", () => {
		const jsonLd = recording(bare, { date: 'last summer' });

		expect(jsonLd).not.toHaveProperty('dateCreated');
		expect(jsonLd).not.toHaveProperty('image');
		expect(recording(project, { date: '2026-10-04 at the boathouse' })).not.toHaveProperty('dateCreated');
	});
});

describe('sets', () => {
	const account = { did: 'did:plc:mira', handle: 'mira.test' };

	const set: Pick<DjSet, 'uri' | 'title' | 'venue' | 'location' | 'date' | 'tracks'> = {
		uri: 'at://did:plc:mira/computer.localnetwork.dj.set/3kset',
		title: 'Closing',
		date: '2026-09-01',
		venue: 'Halle Nord',
		location: { country: 'DE', locality: 'Berlin' },
		tracks: [
			{ title: 'Night Bus', artists: [{ artistName: 'Ferrule', artistMbId: '1b4f1c3e-0000-4000-8000-000000000001' }] },
			{ title: 'Halogen', artist: 'Two Rooms' },
			{ title: 'Kept back', unidentified: true },
			{ startSeconds: 600 }
		]
	};

	const playlist = (fields: Partial<typeof set> = {}) => {
		const page = setMeta(SITE, account, { name: 'Mira', avatarUrl: null }, { ...set, ...fields });

		return { page, jsonLd: jsonLdFor(page, setPlaylist({ ...set, ...fields }, { name: 'Mira', path: '/@mira.test' })) };
	};

	it("makes a set a playlist by its DJ, naming only the tracks they named, at the set's page", () => {
		const { page, jsonLd } = playlist();

		expect(page).toMatchObject({
			title: 'Closing by Mira',
			description: 'Mira played 4 tracks at Halle Nord, Berlin on Sep 1, 2026.',
			url: 'https://localnetwork.test/@mira.test/sets/3kset',
			type: 'music.playlist'
		});
		expect(jsonLd).toEqual({
			'@context': 'https://schema.org',
			'@type': 'MusicPlaylist',
			name: 'Closing',
			url: 'https://localnetwork.test/@mira.test/sets/3kset',
			dateCreated: '2026-09-01',
			numTracks: 4,
			creator: { '@type': 'Person', name: 'Mira', url: 'https://localnetwork.test/@mira.test' },
			track: [
				{
					'@type': 'MusicRecording',
					name: 'Night Bus',
					byArtist: [
						{
							'@type': 'MusicGroup',
							name: 'Ferrule',
							sameAs: ['https://musicbrainz.org/artist/1b4f1c3e-0000-4000-8000-000000000001']
						}
					]
				},
				{ '@type': 'MusicRecording', name: 'Halogen', byArtist: [{ '@type': 'MusicGroup', name: 'Two Rooms' }] }
			]
		});
	});

	it('names a set without a title by its place, and leaves out what it lacks', () => {
		expect(playlist({ title: undefined }).jsonLd.name).toBe('Halle Nord, Berlin');

		const { page, jsonLd } = playlist({
			title: undefined,
			venue: undefined,
			location: undefined,
			tracks: [{ title: 'Alone' }]
		});

		expect(page).toMatchObject({ title: 'Untitled set by Mira', description: 'Mira played 1 track on Sep 1, 2026.' });
		expect(jsonLd.track).toEqual([{ '@type': 'MusicRecording', name: 'Alone' }]);
		expect(playlist({ tracks: [{ unidentified: true }] }).jsonLd).not.toHaveProperty('track');
	});

	it("previews with the DJ's picture when they have one, else the site's card", () => {
		expect(setMeta(SITE, account, { name: 'Mira', avatarUrl: '/media/did:plc:mira/bafyface' }, set).image).toEqual({
			url: 'https://localnetwork.test/media/did:plc:mira/bafyface',
			alt: "Mira's picture"
		});
		expect(playlist().page.image).toMatchObject({ url: 'https://localnetwork.test/og/site.png' });
	});
});

describe('profiles', () => {
	const profile: ProfileView = {
		did: 'did:plc:mira',
		uri: 'at://did:plc:mira/computer.localnetwork.profile/self',
		cid: 'c',
		avatarUrl: '/media/did:plc:mira/bafyface',
		record: {
			basics: {
				name: 'Mira',
				links: [
					{ kind: 'website', url: 'https://mira.test' },
					{ kind: 'other', url: 'data:text/html,hi' }
				]
			},
			performer: { tagline: 'Deep house all night' },
			createdAt: '2026-01-01T00:00:00Z',
			updatedAt: '2026-03-01T00:00:00Z'
		}
	};

	const account = { did: 'did:plc:mira', handle: 'mira.test' };

	it("makes a person's profile a profile page about them", () => {
		expect(jsonLdFor(carded(profileMeta(SITE, account, profile, 'Mira')), personProfile(profile, 'Mira'))).toEqual({
			'@context': 'https://schema.org',
			'@type': 'ProfilePage',
			url: 'https://localnetwork.test/@mira.test',
			dateCreated: '2026-01-01T00:00:00Z',
			dateModified: '2026-03-01T00:00:00Z',
			mainEntity: {
				'@type': 'Person',
				name: 'Mira',
				url: 'https://localnetwork.test/@mira.test',
				image: 'https://localnetwork.test/media/did:plc:mira/bafyface',
				description: 'Deep house all night',
				sameAs: ['https://mira.test']
			}
		});
	});

	it('leaves out what a person has not filled in', () => {
		const plain: ProfileView = {
			...profile,
			avatarUrl: null,
			record: {
				basics: { name: 'Mira', location: { city: 'Lagos', country: 'NG' } },
				createdAt: '2026-01-01T00:00:00Z',
				updatedAt: '2026-03-01T00:00:00Z'
			}
		};

		expect(
			jsonLdFor(carded(profileMeta(SITE, account, plain, 'Mira')), personProfile(plain, 'Mira'))?.mainEntity
		).toEqual({ '@type': 'Person', name: 'Mira', url: 'https://localnetwork.test/@mira.test' });
	});
});

describe('artists', () => {
	it('makes a credited artist a music group, the same as their MusicBrainz artist', () => {
		const path = '/artists/7d0b9c12-5e64-4f3a-8b21-c4e9a6f05d72';

		expect(
			jsonLdFor(
				artistMeta(SITE, 'Mira Okonkwo', path, true),
				musicBrainzArtist('Mira Okonkwo', '7d0b9c12-5e64-4f3a-8b21-c4e9a6f05d72')
			)
		).toEqual({
			'@context': 'https://schema.org',
			'@type': 'MusicGroup',
			name: 'Mira Okonkwo',
			url: `https://localnetwork.test${path}`,
			sameAs: ['https://musicbrainz.org/artist/7d0b9c12-5e64-4f3a-8b21-c4e9a6f05d72']
		});
	});
});
