import { describe, expect, it } from 'vitest';
import type { ProjectView } from '#lib/types.ts';
import { artistMeta, mixMeta, pageMeta, performerMeta, pressKitMeta, pressKitsMeta, profileMeta } from './meta';

const SITE = 'https://localnetwork.test';

/** One of the site's cards, at `path`. */
const card = (path: string, alt: string) => ({ url: `${SITE}${path}`, alt, width: 1200, height: 630 });

const SITE_CARD = card('/og/site.png', 'Local Network: find local DJs for your show');

const project: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/1',
	rkey: '1',
	cid: 'bafyproject',
	slug: 'neon-tide',
	profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
	managers: [],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:a',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: '/media/did:plc:root/bafyavatar'
};

describe('pageMeta', () => {
	it('makes its addresses absolute against the public URL, whatever host the request came in on', () => {
		expect(
			pageMeta(SITE, {
				title: 'Charts',
				description: 'Most played.',
				path: '/charts',
				image: { path: '/a.png', alt: 'A' }
			})
		).toEqual({
			title: 'Charts',
			description: 'Most played.',
			url: 'https://localnetwork.test/charts',
			type: 'website',
			image: { url: 'https://localnetwork.test/a.png', alt: 'A' }
		});
	});

	it("shows the site's card on a page without a picture of its own", () => {
		expect(pageMeta(SITE, { title: 'Charts', description: 'Most played.', path: '/charts' }).image).toEqual(SITE_CARD);
	});

	it('cuts a long description at a word, and flattens its whitespace', () => {
		const { description } = pageMeta(SITE, { title: 'T', description: `${'word '.repeat(60)}\n end`, path: '/' });

		expect(description.length).toBeLessThanOrEqual(200);
		expect(description).toMatch(/^word( word)+…$/);
		expect(pageMeta(SITE, { title: 'T', description: 'two\n\n  lines ', path: '/' }).description).toBe('two lines');
		// One word longer than the limit is cut where the limit falls.
		expect(pageMeta(SITE, { title: 'T', description: 'x'.repeat(300), path: '/' }).description).toBe(
			`${'x'.repeat(199)}…`
		);
	});
});

describe('performers', () => {
	it('describes a performer by their tagline, or what and where they are, with their card', () => {
		expect(performerMeta(SITE, project)).toEqual({
			title: 'Neon Tide',
			description: 'Solo DJ from Berlin, DE',
			url: 'https://localnetwork.test/projects/neon-tide',
			type: 'profile',
			image: card('/og/project/neon-tide/bafyproject.png', 'Neon Tide, a solo DJ from Berlin'),
			noindex: false
		});
		// A kind's label names it in the card's description, with the article it takes.
		expect(performerMeta(SITE, { ...project, profile: { ...project.profile, kind: 'duo' } }).image?.alt).toBe(
			'Neon Tide, a duo from Berlin'
		);
		expect(performerMeta(SITE, { ...project, profile: { ...project.profile, kind: 'organ trio' } }).image?.alt).toBe(
			'Neon Tide, an organ trio from Berlin'
		);
		expect(
			performerMeta(SITE, { ...project, profile: { ...project.profile, tagline: 'Rolling techno' } }).description
		).toBe('Rolling techno');
	});

	it("keeps pages only their managers can see out of search, with the site's card instead of one that would 404", () => {
		const hidden: ProjectView = { ...project, status: 'hidden' };
		const photo = { image: { ref: { $link: 'bafyphoto' } }, alt: 'Live' };

		expect(performerMeta(SITE, hidden)).toMatchObject({ noindex: true, image: SITE_CARD });
		expect(pressKitsMeta(SITE, { ...project, status: 'archived' })).toMatchObject({
			title: 'Neon Tide press kit',
			url: 'https://localnetwork.test/projects/neon-tide/press-kits',
			type: 'profile',
			noindex: true,
			image: SITE_CARD
		});
		expect(mixMeta(SITE, hidden, { title: 'Late Room' }, 1, 3).image).toEqual(SITE_CARD);
		expect(
			pressKitMeta(SITE, hidden, { rkey: 'k1', cid: 'bafykit', record: { title: 'Kit', photos: [photo] } }).image
		).toEqual(SITE_CARD);
	});

	it("gives a name the cards' fonts can't draw the site's card, rather than a card with no name on it", () => {
		const named = (name: string): ProjectView => ({ ...project, profile: { ...project.profile, name } });

		expect(performerMeta(SITE, named('ネオンタイド')).image).toEqual(SITE_CARD);
		expect(mixMeta(SITE, named('🌊🌊'), { title: 'Late Room' }, 1, 3).image).toEqual(SITE_CARD);
		expect(artistMeta(SITE, 'ネオン', '/artists/2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41', true).image).toEqual(SITE_CARD);
		// Some drawable letters are enough: the rest is left out of the card.
		expect(performerMeta(SITE, named('Neon 🌊 Tide')).image?.url).toMatch(/\/og\/project\//);
		expect(performerMeta(SITE, named('Ñandú')).image?.url).toMatch(/\/og\/project\//);
	});

	it("shows a press kit's card with its first press photo, or the performer's card", () => {
		const kit = (photos?: { image: { ref: { $link: string } }; alt: string }[]) => ({
			rkey: 'k1',
			cid: 'bafykit',
			record: { title: 'Bookings 2026', headline: 'Closing sets', photos }
		});

		const photo = { image: { ref: { $link: 'bafyphoto' } }, alt: 'Neon Tide behind the decks' };

		expect(pressKitMeta(SITE, project, kit([photo]))).toMatchObject({
			title: 'Neon Tide: Bookings 2026',
			description: "Neon Tide's press kit: Closing sets",
			url: 'https://localnetwork.test/projects/neon-tide/press-kits/k1',
			image: card(
				'/og/press-kit/neon-tide/k1/bafykit.png',
				'Neon Tide, Closing sets, over their press photo: Neon Tide behind the decks'
			)
		});
		expect(pressKitMeta(SITE, project, kit()).image).toEqual(performerMeta(SITE, project).image);

		// Without a headline, the kit says what the performer is.
		const plain = pressKitMeta(SITE, project, {
			rkey: 'k2',
			cid: 'bafykit',
			record: { title: 'Kit', photos: [{ image: { ref: { $link: 'bafyphoto' } }, alt: '' }] }
		});

		expect(plain.description).toBe("Neon Tide's press kit: Solo DJ from Berlin, DE");
		expect(plain.image?.alt).toBe('Neon Tide, over their press photo');
	});

	it('names a mix by its performer and counts its tracks', () => {
		expect(mixMeta(SITE, project, { title: 'Late Room' }, 2, 31)).toMatchObject({
			title: 'Late Room by Neon Tide',
			description: 'The tracklist of Late Room by Neon Tide: 31 tracks, when each plays, and how they were mixed.',
			url: 'https://localnetwork.test/projects/neon-tide/mixes/2',
			type: 'music.song',
			image: card('/og/mix/neon-tide/2/bafyproject.png', "Late Room by Neon Tide, 31 tracks, with the mix's waveform")
		});
	});
});

describe('profiles', () => {
	const account = { did: 'did:plc:neon', handle: 'neon.test' };

	const view = (
		record: { basics?: { location?: { city: string; country: string } }; performer?: { tagline?: string } },
		avatarUrl: string | null = null
	) => ({ record: { basics: {}, ...record }, avatarUrl });

	it("lives at the account's handle and says what the performer does, or where they are", () => {
		expect(
			profileMeta(SITE, account, view({ performer: { tagline: 'Rolling techno' } }, '/media/did:plc:neon/b'), 'Neon')
		).toEqual({
			title: 'Neon',
			description: 'Rolling techno',
			url: 'https://localnetwork.test/@neon.test',
			type: 'profile',
			image: { url: 'https://localnetwork.test/media/did:plc:neon/b', alt: "Neon's picture" }
		});
		expect(
			profileMeta(SITE, account, view({ basics: { location: { city: 'Berlin', country: 'DE' } } }), 'Neon').description
		).toBe('Neon, Berlin, DE');
	});

	it("has only a name and the site's card for an account without a profile, at its DID when it has no handle", () => {
		expect(profileMeta(SITE, { did: 'did:plc:neon', handle: null }, null, '@neon.bsky.social')).toMatchObject({
			description: '@neon.bsky.social',
			url: 'https://localnetwork.test/@did:plc:neon',
			image: SITE_CARD
		});
	});
});

describe('artists', () => {
	it("names the artist when the directory knows them, and the page's canonical address either way", () => {
		const mbid = '2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41';

		expect(artistMeta(SITE, 'Ferrule', `/artists/${mbid}`, true)).toEqual({
			title: 'Ferrule',
			description: "Which Local Network DJs played Ferrule's tracks, and where.",
			url: `https://localnetwork.test/artists/${mbid}`,
			type: 'website',
			image: card(`/og/artist/${mbid}.png`, 'Ferrule: played by Local Network DJs, with their most played tracks'),
			noindex: false
		});
		expect(artistMeta(SITE, null, `/artists/${mbid}`, false)).toMatchObject({
			title: 'Artist',
			description: "Which Local Network DJs played this artist's tracks, and where.",
			image: SITE_CARD
		});
		// An artist credited only by name has no card of their own.
		expect(artistMeta(SITE, 'Ferrule', '/artists/name/Ferrule', true).image).toEqual(SITE_CARD);
	});

	it('keeps an artist no set credits out of search, since anything typed into the address names it', () => {
		expect(artistMeta(SITE, 'cheap pills here', '/artists/name/cheap%20pills%20here', false).noindex).toBe(true);
	});
});
