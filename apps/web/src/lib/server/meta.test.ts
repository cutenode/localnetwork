import { describe, expect, it } from 'vitest';
import type { ProjectView } from '#lib/types.ts';
import { artistMeta, mixMeta, pageMeta, performerMeta, pressKitMeta, pressKitsMeta, profileMeta } from './meta';

const SITE = 'https://localnetwork.test';

const project: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/1',
	rkey: '1',
	cid: 'c',
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
	it('describes a performer by their tagline, or what and where they are, with their picture', () => {
		expect(performerMeta(SITE, project)).toEqual({
			title: 'Neon Tide',
			description: 'Solo DJ from Berlin, DE',
			url: 'https://localnetwork.test/projects/neon-tide',
			type: 'profile',
			image: { url: 'https://localnetwork.test/media/did:plc:root/bafyavatar', alt: "Neon Tide's picture" },
			noindex: false
		});
		expect(
			performerMeta(SITE, { ...project, profile: { ...project.profile, tagline: 'Rolling techno' } }).description
		).toBe('Rolling techno');
	});

	it("keeps pages only their managers can see out of search, and leaves out a picture they don't have", () => {
		const hidden = performerMeta(SITE, { ...project, status: 'hidden', avatarUrl: null });

		expect(hidden.noindex).toBe(true);
		expect(hidden.image).toBeUndefined();
		expect(pressKitsMeta(SITE, { ...project, status: 'archived' })).toMatchObject({
			title: 'Neon Tide press kit',
			url: 'https://localnetwork.test/projects/neon-tide/press-kits',
			type: 'profile',
			noindex: true
		});
	});

	it("shows a press kit's first press photo, or the performer's picture", () => {
		const kit = (photos?: { image: { ref: { $link: string } }; alt: string }[]) => ({
			rkey: 'k1',
			record: { title: 'Bookings 2026', headline: 'Closing sets', photos }
		});

		const photo = { image: { ref: { $link: 'bafyphoto' } }, alt: 'Neon Tide behind the decks' };

		expect(pressKitMeta(SITE, project, kit([photo]))).toMatchObject({
			title: 'Neon Tide: Bookings 2026',
			description: "Neon Tide's press kit: Closing sets",
			url: 'https://localnetwork.test/projects/neon-tide/press-kits/k1',
			image: {
				url: 'https://localnetwork.test/projects/neon-tide/press-kits/k1/photos/bafyphoto',
				alt: 'Neon Tide behind the decks'
			}
		});
		expect(pressKitMeta(SITE, project, kit()).image?.alt).toBe("Neon Tide's picture");
		// Without a headline, the kit says what the performer is.
		expect(pressKitMeta(SITE, project, { rkey: 'k2', record: { title: 'Kit' } }).description).toBe(
			"Neon Tide's press kit: Solo DJ from Berlin, DE"
		);
	});

	it('names a mix by its performer and counts its tracks', () => {
		expect(mixMeta(SITE, project, { title: 'Late Room' }, 2, 31)).toMatchObject({
			title: 'Late Room by Neon Tide',
			description: 'The tracklist of Late Room by Neon Tide: 31 tracks, when each plays, and how they were mixed.',
			url: 'https://localnetwork.test/projects/neon-tide/mixes/2',
			type: 'music.song'
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

	it('has only a name for an account without a profile, at its DID when it has no handle', () => {
		expect(profileMeta(SITE, { did: 'did:plc:neon', handle: null }, null, '@neon.bsky.social')).toMatchObject({
			description: '@neon.bsky.social',
			url: 'https://localnetwork.test/@did:plc:neon'
		});
	});
});

describe('artists', () => {
	it("names the artist when the directory knows them, and the page's canonical address either way", () => {
		expect(artistMeta(SITE, 'Ferrule', '/artists/2a8e4f60', true)).toEqual({
			title: 'Ferrule',
			description: "Which Local Network DJs played Ferrule's tracks, and where.",
			url: 'https://localnetwork.test/artists/2a8e4f60',
			type: 'website',
			noindex: false
		});
		expect(artistMeta(SITE, null, '/artists/2a8e4f60', false)).toMatchObject({
			title: 'Artist',
			description: "Which Local Network DJs played this artist's tracks, and where."
		});
	});

	it('keeps an artist no set credits out of search, since anything typed into the address names it', () => {
		expect(artistMeta(SITE, 'cheap pills here', '/artists/name/cheap%20pills%20here', false).noindex).toBe(true);
	});
});
