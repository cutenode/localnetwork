// src/tools/artists/profiles.ts — who is behind a track, and where else to
// find them: the pure logic under the artists panel.
import { describe, it, expect } from 'vitest';
import {
	artistPathOf,
	artistsOf,
	asText,
	bioLinks,
	hasProfile,
	isArtistPath,
	isUnavailable,
	isUnchecked,
	linksOf,
	nameOf,
	NETWORKS,
	networkOf,
	placeOf,
	profileLine,
	profileUrl,
	rowFacts,
	searchText,
	shortCount,
	SORT_VALUES,
	SORTS,
	sorted
} from '../../src/tools/artists/profiles.ts';

const profile = (extra = {}) => ({
	id: 7,
	permalink: '/bicep',
	username: 'Bicep',
	fullName: 'Andy & Matt',
	city: 'Belfast',
	country: 'GB',
	followers: 1234,
	trackCount: 42,
	verified: true,
	avatarUrl: 'https://i1.sndcdn.com/avatars-abc-large.jpg',
	bio: '',
	links: [],
	at: 1,
	...extra
});

const entry = (extra = {}) => ({
	permalink: '/bicep',
	name: 'Bicep',
	tracks: [{ permalink: '/bicep/glue', title: 'Glue' }],
	profile: null,
	from: 'My Playlist',
	at: 1,
	...extra
});

describe('an artist’s path', () => {
	it('is the first segment of a track’s permalink', () => {
		expect(artistPathOf('/bicep/glue')).toBe('/bicep');
		expect(artistPathOf('/four-tet/baby_2')).toBe('/four-tet');
	});

	it('is nothing for anything that is not a track’s permalink', () => {
		for (const bad of ['', '/bicep', '/a/b/c', 'bicep/glue', '/a/b?in=c', null, undefined]) {
			expect(artistPathOf(bad), String(bad)).toBe('');
		}

		// @ts-expect-error -- a permalink out of storage an older version wrote may be anything at all
		expect(artistPathOf(7), '7').toBe('');
	});

	it('is one segment and nothing else', () => {
		expect(isArtistPath('/bicep')).toBe(true);

		for (const bad of ['/bicep/glue', 'bicep', '/', '/a b', '/a?x', '', null]) {
			expect(isArtistPath(bad), String(bad)).toBe(false);
		}
	});

	it('opens the profile page', () => {
		expect(profileUrl('/bicep')).toBe('https://soundcloud.com/bicep');
	});
});

describe('the artists behind a list of tracks', () => {
	const track = (artist: string, title: string, permalink: string) => ({
		artist,
		title,
		permalink
	});

	it('is one per uploader, in the order first seen, with the tracks that brought it in', () => {
		expect(
			artistsOf([
				track('Bicep', 'Glue', '/bicep/glue'),
				track('Four Tet', 'Baby', '/fourtet/baby'),
				track('Bicep', 'Aura', '/bicep/aura')
			])
		).toEqual([
			{
				permalink: '/bicep',
				name: 'Bicep',
				tracks: [
					{ permalink: '/bicep/glue', title: 'Glue' },
					{ permalink: '/bicep/aura', title: 'Aura' }
				]
			},
			{ permalink: '/fourtet', name: 'Four Tet', tracks: [{ permalink: '/fourtet/baby', title: 'Baby' }] }
		]);
	});

	it('skips a track with no permalink, and counts the same track once', () => {
		expect(
			artistsOf([
				track('Nobody', 'Nothing', ''),
				track('Bicep', 'Glue', '/bicep/glue'),
				track('Bicep', 'Glue', '/bicep/glue'),
				// @ts-expect-error -- a null among the tracks
				null,
				// @ts-expect-error -- a track with nothing on it, not even a permalink
				{}
			])
		).toEqual([{ permalink: '/bicep', name: 'Bicep', tracks: [{ permalink: '/bicep/glue', title: 'Glue' }] }]);
	});

	it('takes a name from whichever row gave one', () => {
		expect(artistsOf([track('', 'Glue', '/bicep/glue'), track(' Bicep ', '', '/bicep/aura')])).toEqual([
			{
				permalink: '/bicep',
				name: 'Bicep',
				tracks: [
					{ permalink: '/bicep/glue', title: 'Glue' },
					{ permalink: '/bicep/aura', title: '' }
				]
			}
		]);
	});

	it('is nothing for anything that is not a list', () => {
		// @ts-expect-error -- no list at all
		expect(artistsOf(null)).toEqual([]);
		// @ts-expect-error -- a string where the list belongs
		expect(artistsOf('tracks')).toEqual([]);
	});

	it('names an artist by the page’s word, else SoundCloud’s, else the path', () => {
		expect(nameOf(entry())).toBe('Bicep');
		expect(nameOf(entry({ name: '', profile: profile({ username: 'BICEP' }) }))).toBe('BICEP');
		expect(nameOf(entry({ name: '', profile: null }))).toBe('bicep');
	});
});

describe('which network an address is on', () => {
	it('is read off the host, whatever SoundCloud filed it under', () => {
		expect(networkOf('https://www.instagram.com/bicepmusic/', 'personal')).toBe('instagram');
		expect(networkOf('https://x.com/bicep')).toBe('twitter');
		expect(networkOf('https://twitter.com/bicep')).toBe('twitter');
		expect(networkOf('https://open.spotify.com/artist/abc')).toBe('spotify');
		expect(networkOf('https://music.apple.com/gb/artist/bicep/1')).toBe('apple');
		expect(networkOf('https://bicep.bandcamp.com/')).toBe('bandcamp');
		expect(networkOf('https://ra.co/dj/bicep')).toBe('residentadvisor');
		expect(networkOf('https://youtu.be/abc')).toBe('youtube');
		expect(networkOf('https://linktr.ee/bicep')).toBe('linktree');
		expect(networkOf('https://soundcloud.com/bicep')).toBe('soundcloud');
	});

	it('falls back to what SoundCloud filed it under, for a host it does not know', () => {
		expect(networkOf('https://example.com/bicep', 'tumblr')).toBe('personal');
		expect(networkOf('https://example.com/bicep', 'Tiktok')).toBe('tiktok');
		expect(networkOf('https://example.com/bicep', 'email')).toBe('personal');
		expect(networkOf('https://example.com/bicep')).toBe('personal');
		expect(networkOf('not a url', 'instagram')).toBe('instagram');
	});

	it('is email for a mailto, whatever it was filed under', () => {
		expect(networkOf('mailto:a@b.com', 'personal')).toBe('email');
	});

	it('does not take a host that merely ends with a network’s name', () => {
		expect(networkOf('https://notinstagram.com/x')).toBe('personal');
	});

	it('describes every network with a name, a badge and a colour', () => {
		for (const [id, network] of Object.entries(NETWORKS)) {
			expect(network.name, id).toBeTruthy();
			expect(network.badge.length, id).toBeLessThanOrEqual(3);
			expect(network.background, id).toMatch(/^#[0-9a-f]{6}$/);
			expect(Array.isArray(network.hosts), id).toBe(true);
		}
	});
});

describe('the addresses written into a bio', () => {
	it('finds full addresses, www ones, known hosts with a path, and emails', () => {
		const links = bioLinks(
			'IG: https://www.instagram.com/bicepmusic/ — also www.feelmybicep.com and ' +
				'linktr.ee/bicep. bookings: agent@example.com!'
		);

		expect(links.map((link) => [link.network, link.url, link.source])).toEqual([
			['instagram', 'https://www.instagram.com/bicepmusic/', 'bio'],
			['personal', 'https://www.feelmybicep.com', 'bio'],
			['linktree', 'https://linktr.ee/bicep', 'bio'],
			['email', 'mailto:agent@example.com', 'bio']
		]);
		expect(links[1].name).toBe('feelmybicep.com');
		expect(links[3].name).toBe('email');
	});

	it('leaves a bare word with a dot in it alone, and finds nothing in nothing', () => {
		expect(bioLinks('e.g. this vs. that, bicep.com too')).toEqual([]);
		expect(bioLinks('')).toEqual([]);
		// @ts-expect-error -- null rather than a bio, which is nothing to search either
		expect(bioLinks(null)).toEqual([]);
	});

	it('drops the punctuation a sentence ends an address with', () => {
		expect(bioLinks('see https://example.com/page, www.bicep.net; and linktr.ee/bicep!').map((l) => l.url)).toEqual([
			'https://example.com/page',
			'https://www.bicep.net',
			'https://linktr.ee/bicep'
		]);
	});
});

describe('every link an artist offers', () => {
	it('is the profile’s links first, then the bio’s, one per address', () => {
		const links = linksOf(
			profile({
				links: [
					{ network: 'instagram', title: 'IG', url: 'https://www.instagram.com/bicepmusic/' },
					{ network: 'personal', title: 'site', url: 'https://feelmybicep.com/' }
				],
				bio: 'instagram.com/bicepmusic and https://FEELMYBICEP.com plus https://x.com/bicep'
			})
		);

		expect(links.map((link) => [link.network, link.source, link.title])).toEqual([
			['instagram', 'profile', 'IG'],
			['personal', 'profile', 'site'],
			['twitter', 'bio', '']
		]);
		expect(links[0]).toMatchObject({
			key: 'instagram.com/bicepmusic',
			badge: 'IG',
			style: 'background:#e1306c;color:#ffffff'
		});
	});

	it('skips a profile link with no address, and copes with links that are not a list', () => {
		expect(linksOf(profile({ links: [{ url: '' }, null, { url: 'https://x.com/a' }] }))).toHaveLength(1);
		expect(linksOf(profile({ links: 'nope' }))).toEqual([]);
	});

	it('is nothing for nobody asked, and for no public profile', () => {
		expect(linksOf(null)).toEqual([]);
		expect(linksOf({ unavailable: true, at: 1 })).toEqual([]);
	});
});

describe('what a row says', () => {
	it('knows the three states an artist can be in', () => {
		expect(isUnchecked(entry())).toBe(true);
		expect(isUnavailable(entry({ profile: { unavailable: true, at: 1 } }))).toBe(true);
		expect(hasProfile(entry({ profile: profile() }))).toBe(true);
		expect(hasProfile(entry({ profile: { unavailable: true, at: 1 } }))).toBe(false);
		expect(isUnchecked(entry({ profile: profile() }))).toBe(false);
	});

	it('shortens a follower count to the width a row has', () => {
		expect(shortCount(0)).toBe('0');
		expect(shortCount(999)).toBe('999');
		expect(shortCount(1000)).toBe('1K');
		expect(shortCount(1234)).toBe('1.2K');
		expect(shortCount(12345)).toBe('12K');
		expect(shortCount(999999)).toBe('1000K');
		expect(shortCount(1234567)).toBe('1.2M');
		expect(shortCount(34000000)).toBe('34M');
		expect(shortCount(null)).toBe('');
		expect(shortCount(-1)).toBe('');
		expect(shortCount(1.5)).toBe('');
	});

	it('names a place from whichever halves are there', () => {
		expect(placeOf(profile())).toBe('Belfast, GB');
		expect(placeOf(profile({ city: '' }))).toBe('GB');
		expect(placeOf(profile({ country: '' }))).toBe('Belfast');
		expect(placeOf(profile({ city: '', country: '' }))).toBe('');
		expect(placeOf(null)).toBe('');
	});

	it('says what is known, or why nothing is', () => {
		expect(profileLine(entry())).toBe('not looked up');
		expect(profileLine(entry(), { checking: true })).toBe('looking up…');
		expect(profileLine(entry({ profile: { unavailable: true, at: 1 } }))).toBe('no public profile');
		expect(profileLine(entry({ profile: profile() }))).toBe('Belfast, GB · 1.2K followers · no links');
		expect(
			profileLine(
				entry({
					profile: profile({
						city: '',
						country: '',
						followers: null,
						links: [{ url: 'https://x.com/a' }]
					})
				})
			)
		).toBe('1 link');
		expect(profileLine(entry({ profile: profile({ bio: 'https://x.com/a and https://instagram.com/a' }) }))).toBe(
			'Belfast, GB · 1.2K followers · 2 links'
		);
	});

	// A state file can restore a profile as little as `{ at }`. A follower count
	// it does not carry is one SoundCloud did not give, as null is.
	it('says nothing of followers a restored profile does not carry', () => {
		const restored = entry({ profile: { at: 1 } });
		expect(profileLine(restored)).toBe('no links');
		expect(profileLine(restored)).toBe(profileLine(entry({ profile: { followers: null, at: 1 } })));
	});

	it('is searched by everything known about the artist', () => {
		expect(searchText(entry())).toBe('Bicep /bicep My Playlist Glue');

		const full = searchText(
			entry({
				profile: profile({
					bio: 'bookings: agent@example.com',
					links: [{ network: 'instagram', title: 'IG', url: 'https://instagram.com/bicepmusic' }]
				})
			})
		);

		expect(full).toContain('Belfast');
		expect(full).toContain('Andy & Matt');
		expect(full).toContain('Instagram https://instagram.com/bicepmusic IG');
		expect(full).toContain('email mailto:agent@example.com');
	});
});

describe('what a row opens into', () => {
	it('is the profile, every link, and what the list knows, for an artist nobody asked about', () => {
		expect(rowFacts(entry())).toEqual([
			{
				id: 'soundcloud',
				label: 'SoundCloud',
				value: 'https://soundcloud.com/bicep',
				copy: 'https://soundcloud.com/bicep',
				open: 'https://soundcloud.com/bicep'
			},
			{ id: 'tracks', label: 'tracks', value: 'Glue', copy: 'Glue', open: '' },
			{ id: 'from', label: 'from', value: 'My Playlist', copy: 'My Playlist', open: '' }
		]);
	});

	it('adds what SoundCloud said, with nothing to copy where it said nothing', () => {
		const facts = rowFacts(
			entry({
				name: 'BICEP',
				from: '',
				tracks: [{ permalink: '/bicep/glue', title: '' }],
				profile: profile({
					followers: 1234567,
					links: [{ network: 'instagram', title: 'IG', url: 'https://instagram.com/bicepmusic' }],
					bio: 'hello'
				})
			})
		);

		expect(facts.map((fact) => [fact.id, fact.value, fact.copy])).toEqual([
			['soundcloud', 'https://soundcloud.com/bicep', 'https://soundcloud.com/bicep'],
			['link:instagram.com/bicepmusic', 'https://instagram.com/bicepmusic', 'https://instagram.com/bicepmusic'],
			['username', 'Bicep', 'Bicep'],
			['place', 'Belfast, GB', 'Belfast, GB'],
			['followers', '1,234,567', '1,234,567'],
			['bio', 'hello', 'hello'],
			['tracks', '/bicep/glue', '/bicep/glue'],
			['from', 'unknown', '']
		]);
		expect(facts[1]).toMatchObject({ label: 'Instagram', open: 'https://instagram.com/bicepmusic', hint: 'IG' });
	});

	it('leaves out SoundCloud’s name when it is the page’s, and says what it does not know', () => {
		const facts = rowFacts(entry({ profile: profile({ city: '', country: '', followers: null, bio: '' }) }));
		expect(facts.map((fact) => fact.id)).toEqual(['soundcloud', 'place', 'followers', 'bio', 'tracks', 'from']);
		expect(facts.find((fact) => fact.id === 'place')).toMatchObject({ value: 'unknown', copy: '' });
		expect(facts.find((fact) => fact.id === 'followers')).toMatchObject({ value: 'unknown', copy: '' });
	});

	it('does not know the followers of a restored profile that does not carry them', () => {
		const facts = rowFacts(entry({ profile: { at: 1 } }));
		expect(facts.find((fact) => fact.id === 'followers')).toMatchObject({ value: 'unknown', copy: '' });
		expect(facts).toEqual(rowFacts(entry({ profile: { followers: null, at: 1 } })));
	});
});

describe('the list in order', () => {
	const a = entry({ permalink: '/a', name: 'Zed', at: 1 });
	const b = entry({ permalink: '/b', name: 'álvaro', at: 2, profile: profile({ followers: 10 }) });
	const c = entry({ permalink: '/c', name: 'Bicep', at: 3, profile: profile({ followers: null }) });
	const d = entry({ permalink: '/d', name: '', at: 4, profile: profile({ followers: 99, username: 'Dee' }) });

	it('offers by name, list order and most followed', () => {
		expect(SORTS.map((sort) => sort.value)).toEqual(['name', 'list', 'followers']);
		expect(SORT_VALUES).toEqual(['name', 'list', 'followers']);
	});

	it('sorts by name without minding case or accents, and leaves the list as it came', () => {
		expect(sorted([a, b, c, d], 'name').map((one) => one.permalink)).toEqual(['/b', '/c', '/d', '/a']);
		expect(sorted([a, b, c, d], 'list').map((one) => one.permalink)).toEqual(['/a', '/b', '/c', '/d']);
		// A copy, sorted: the list it was handed is the store's, and stays put.
		const input = [a, b, c, d];
		expect(sorted(input, 'list')).not.toBe(input);
		sorted(input, 'name');
		expect(input).toEqual([a, b, c, d]);
	});

	it('puts the most followed first, and the ones nobody counted last', () => {
		expect(sorted([a, b, c, d], 'followers').map((one) => one.permalink)).toEqual(['/d', '/b', '/a', '/c']);
	});
});

describe('the list as text', () => {
	it('is a block per artist: the name, the profile, then every link', () => {
		expect(
			asText([
				entry({
					profile: profile({
						links: [{ network: 'instagram', title: '', url: 'https://instagram.com/bicepmusic' }],
						bio: 'agent@example.com'
					})
				}),
				entry({ permalink: '/fourtet', name: 'Four Tet' })
			])
		).toBe(
			[
				'Bicep',
				'https://soundcloud.com/bicep',
				'Instagram: https://instagram.com/bicepmusic',
				'email: mailto:agent@example.com',
				'',
				'Four Tet',
				'https://soundcloud.com/fourtet'
			].join('\n')
		);
		expect(asText([])).toBe('');
	});
});
