import type { JsonValue } from '@atproto/lex-json';
import { describe, expect, it } from 'vitest';
import { sampleReport } from '#lib/mix/sample.ts';
import type { BlobRefJson } from '#lib/types.ts';
import { parseAvatar, parseDetails, parseProfile, profileToValues, sniffImage, withTracklists } from './project';
import { formValues } from '#lib/project-form.ts';

function form(entries: [string, string | File][]): FormData {
	const data = new FormData();

	for (const [key, value] of entries) data.append(key, value);

	return data;
}

const minimal: [string, string][] = [
	['name', 'Neon Tide'],
	['kind', 'solo'],
	['genres', 'Techno, dub techno'],
	['city', 'Berlin'],
	['country', 'de']
];

describe('parseProfile', () => {
	it('parses a minimal profile and normalizes fields', () => {
		expect(parseProfile(form(minimal))).toEqual({
			name: 'Neon Tide',
			kind: 'solo',
			genres: ['techno', 'dub techno'],
			location: { city: 'Berlin', country: 'DE' }
		});
	});

	it('parses every optional section', () => {
		const profile = parseProfile(
			form([
				...minimal,
				['tagline', 'Hypnotic'],
				['bio', 'Long bio'],
				['pronouns', 'they/them'],
				['region', 'Berlin'],
				['latE6', '52520000'],
				['lngE6', '13405000'],
				['travelRadiusKm', '300'],
				['international', 'on'],
				['travelNotes', 'Has a car'],
				['formats', 'vinyl'],
				['formats', 'bogus'],
				['setTypes', 'peak'],
				['setMin', '60'],
				['setMax', '240'],
				['activeSince', '2015'],
				['capacityMin', '100'],
				['capacityMax', '900'],
				['feeLabel', ' Club nights '],
				['feeTier', 'established'],
				['feeCurrency', 'eur'],
				['feeMin', '500'],
				['feeMax', '1500'],
				['linkKind', 'soundcloud'],
				['linkUrl', 'soundcloud.com/neon'],
				['linkKind', 'bogus'],
				['linkUrl', 'https://neon.example'],
				['mixTitle', ''],
				['mixUrl', 'https://mixcloud.com/neon/one'],
				['highlights', 'Residency\n\nFestival'],
				['lookingFor', 'Late slots']
			])
		);

		expect(profile).toMatchObject({
			location: { region: 'Berlin', latE6: 52520000, lngE6: 13405000 },
			travel: { radiusKm: 300, international: true, notes: 'Has a car' },
			formats: ['vinyl'],
			setTypes: ['peak'],
			setLength: { minMinutes: 60, maxMinutes: 240 },
			activeSince: 2015,
			capacity: { min: 100, max: 900 },
			fees: [{ label: 'Club nights', tier: 'established', currency: 'EUR', minAmount: 500, maxAmount: 1500 }],
			links: [
				{ kind: 'soundcloud', url: 'https://soundcloud.com/neon' },
				{ kind: 'other', url: 'https://neon.example/' }
			],
			mixes: [{ title: 'Mix', url: 'https://mixcloud.com/neon/one' }],
			highlights: ['Residency', 'Festival'],
			lookingFor: 'Late slots'
		});
	});

	it('keeps the first of each repeated highlight, link, and mix', () => {
		const profile = parseProfile(
			form([
				...minimal,
				['linkKind', 'website'],
				['linkUrl', 'neon.example'],
				['linkKind', 'other'],
				['linkUrl', 'https://neon.example/'],
				['mixTitle', 'One'],
				['mixUrl', 'https://m.test/1'],
				['mixTitle', 'One again'],
				['mixUrl', 'https://m.test/1'],
				['highlights', 'Residency\nFestival\n Residency']
			])
		);

		expect(profile.links).toEqual([{ kind: 'website', url: 'https://neon.example/' }]);
		expect(profile.mixes).toEqual([{ title: 'One', url: 'https://m.test/1' }]);
		expect(profile.highlights).toEqual(['Residency', 'Festival']);
	});

	it.each<[[string, string][], string]>([
		[[['name', '']], 'name'],
		[minimal.filter(([k]) => k !== 'genres'), 'genre'],
		[minimal.filter(([k]) => k !== 'city'), 'city'],
		[[...minimal.filter(([k]) => k !== 'country'), ['country', 'Germany']], 'two-letter'],
		[[...minimal, ['kind', 'band']], 'kind'],
		[[...minimal, ['setMin', '200'], ['setMax', '100']], 'Shortest set'],
		[[...minimal, ['capacityMin', '500'], ['capacityMax', '100']], 'Smallest room'],
		[[...minimal, ['feeTier', 'emerging'], ['feeMin', '9'], ['feeMax', '1']], 'Minimum fee'],
		[[...minimal, ['feeTier', 'free']], 'fee tier'],
		[[...minimal, ['travelRadiusKm', 'far']], 'travelRadiusKm'],
		[[...minimal, ['linkUrl', 'javascript:alert(1)']], 'not a valid link'],
		[[...minimal, ['linkUrl', 'ftp://files.example']], 'web link'],
		[[...minimal, ['tagline', 'x'.repeat(141)]], 'Tagline']
	])('rejects bad input (%#: %s)', (entries, message) => {
		const data = new FormData();
		const seen = new Set<string>();

		for (const [key, value] of entries.toReversed()) {
			if (seen.has(key) && !['linkUrl', 'formats'].includes(key)) continue;
			seen.add(key);
			data.append(key, value);
		}

		expect(() => parseProfile(data)).toThrow(new RegExp(message, 'i'));
	});

	it('keeps link labels and mix lengths and years', () => {
		const profile = parseProfile(
			form([
				...minimal,
				['linkKind', 'website'],
				['linkUrl', 'https://a.test'],
				['linkLabel', 'Shop'],
				['linkKind', 'website'],
				['linkUrl', 'https://b.test'],
				['linkLabel', ''],
				['mixTitle', 'One'],
				['mixUrl', 'https://m.test/1'],
				['mixDuration', '90'],
				['mixYear', '2024'],
				['mixTitle', 'Two'],
				['mixUrl', 'https://m.test/2'],
				['mixDuration', ''],
				['mixYear', '']
			])
		);

		expect(profile.links).toEqual([
			{ kind: 'website', url: 'https://a.test/', label: 'Shop' },
			{ kind: 'website', url: 'https://b.test/' }
		]);
		expect(profile.mixes).toEqual([
			{ title: 'One', url: 'https://m.test/1', durationMinutes: 90, year: 2024 },
			{ title: 'Two', url: 'https://m.test/2' }
		]);
		const link = (label: string) => form([...minimal, ['linkUrl', 'https://a.test'], ['linkLabel', label]]);
		expect(() => parseProfile(link('x'.repeat(65)))).toThrow(/label/);
		expect(() => parseProfile(form([...minimal, ['mixUrl', 'https://m.test'], ['mixYear', '1800']]))).toThrow(
			/mixYear/
		);
	});

	it("links each mix to the set picked on its row, and refuses anything that isn't one", () => {
		const set = { uri: 'at://did:plc:neon/computer.localnetwork.dj.set/3kset', cid: 'bafyreiaaaaaaaaaaaaaaaaaaaaaaa' };

		const mixes = (...sets: string[]) =>
			form([
				...minimal,
				...sets.flatMap((value, index): [string, string][] => [
					['mixTitle', `Mix ${index}`],
					['mixUrl', `https://m.test/${index}`],
					['mixSet', value]
				])
			]);

		expect(parseProfile(mixes('', `${set.uri} ${set.cid}`)).mixes).toEqual([
			{ title: 'Mix 0', url: 'https://m.test/0' },
			{ title: 'Mix 1', url: 'https://m.test/1', set }
		]);

		for (const value of [
			set.uri,
			`at://did:plc:neon/computer.localnetwork.project/3kset ${set.cid}`,
			`${set.uri} not-a-cid`,
			`https://pds.test ${set.cid}`
		])
			expect(() => parseProfile(mixes(value))).toThrow(/Pick a set/);
	});

	it('refuses fee amounts without a tier and bad currency codes', () => {
		expect(() => parseProfile(form([...minimal, ['feeMin', '100']]))).toThrow(/fee tier/);
		expect(() => parseProfile(form([...minimal, ['feeCurrency', 'EUR']]))).toThrow(/fee tier/);
		expect(() => parseProfile(form([...minimal, ['feeLabel', 'Festivals'], ['feeTier', '']]))).toThrow(/fee tier/);
		expect(() => parseProfile(form([...minimal, ['feeTier', 'emerging'], ['feeLabel', 'x'.repeat(65)]]))).toThrow(
			/fee label/
		);

		for (const currency of ['EU', 'EURO', '€'])
			expect(() => parseProfile(form([...minimal, ['feeTier', 'emerging'], ['feeCurrency', currency]]))).toThrow(
				/three-letter currency/
			);
		expect(parseProfile(form([...minimal, ['feeTier', 'emerging'], ['feeCurrency', 'usd']])).fees).toEqual([
			{ tier: 'emerging', currency: 'USD' }
		]);
	});

	it('keeps every filled fee row, in order, and skips empty ones', () => {
		const row = (label: string, tier: string, currency = '', min = '', max = ''): [string, string][] => [
			['feeLabel', label],
			['feeTier', tier],
			['feeCurrency', currency],
			['feeMin', min],
			['feeMax', max]
		];

		const levels = Array.from({ length: 20 }, (_, index) => row(`Level ${index}`, 'emerging')).flat();

		expect(
			parseProfile(
				form([
					...minimal,
					...row('Club nights', 'established', 'eur', '600', '1500'),
					...row('', ''),
					...row('Festivals', 'headliner', 'EUR', '2000'),
					...row('', 'community')
				])
			).fees
		).toEqual([
			{ label: 'Club nights', tier: 'established', currency: 'EUR', minAmount: 600, maxAmount: 1500 },
			{ label: 'Festivals', tier: 'headliner', currency: 'EUR', minAmount: 2000 },
			{ tier: 'community' }
		]);
		expect(parseProfile(form([...minimal, ...levels])).fees).toHaveLength(20);
		expect(parseProfile(form([...minimal, ...row('', '')])).fees).toBeUndefined();
	});

	it('refuses more than 12 highlights', () => {
		const lines = Array.from({ length: 13 }, (_, index) => `Highlight ${index}`);
		expect(() => parseProfile(form([...minimal, ['highlights', lines.join('\n')]]))).toThrow(/12 highlights/);
		expect(parseProfile(form([...minimal, ['highlights', lines.slice(1).join('\n')]])).highlights).toHaveLength(12);
	});
});

describe('parseDetails', () => {
	it('parses private details and validates email', () => {
		expect(
			parseDetails(
				form([
					['bookingEmail', 'a@b.test'],
					['bookingPhone', '+1'],
					['rateNotes', 'r'],
					['techRider', 't'],
					['hospitality', 'h']
				])
			)
		).toEqual({
			bookingEmail: 'a@b.test',
			bookingPhone: '+1',
			rateNotes: 'r',
			techRider: 't',
			hospitality: 'h'
		});
		expect(parseDetails(form([]))).toEqual({});
		expect(() => parseDetails(form([['bookingEmail', 'nope']]))).toThrow(/email/);
	});
});

describe('parseAvatar', () => {
	const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0]);
	it('accepts real images and rejects disguised or oversized files', async () => {
		expect(await parseAvatar(form([]))).toBeUndefined();
		expect(await parseAvatar(form([['avatar', new File([png], 'a.png', { type: 'image/png' })]]))).toMatchObject({
			mimeType: 'image/png'
		});
		await expect(parseAvatar(form([['avatar', new File(['GIF89a'], 'a.gif', { type: 'image/gif' })]]))).rejects.toThrow(
			/PNG, JPEG, or WebP/
		);
		await expect(parseAvatar(form([['avatar', new File(['<svg/>'], 'a.png', { type: 'image/png' })]]))).rejects.toThrow(
			/isn't the image type/
		);
		await expect(
			parseAvatar(form([['avatar', new File([new Uint8Array(1_000_001)], 'a.png', { type: 'image/png' })]]))
		).rejects.toThrow(/1 MB/);
	});
	it('sniffs magic bytes', () => {
		expect(sniffImage(new Uint8Array([0xff, 0xd8, 0xff, 0xe0]), 'image/jpeg')).toBe(true);
		expect(sniffImage(new Uint8Array([...Buffer.from('RIFF'), 0, 0, 0, 0, ...Buffer.from('WEBP')]), 'image/webp')).toBe(
			true
		);
		expect(sniffImage(new Uint8Array([1]), 'image/bmp')).toBe(false);
	});
});

describe('withTracklists', () => {
	const kept: BlobRefJson = { $type: 'blob', ref: { $link: 'bafkept' }, mimeType: 'application/json', size: 10 };
	const json = (value: JsonValue) => new File([JSON.stringify(value)], 'mix.json', { type: 'application/json' });
	const empty = () => new File([], '');

	const rows = (...mixes: [string, string, File, string][]) =>
		form([
			...minimal,
			...mixes.flatMap(([title, url, file, cid]): [string, string | File][] => [
				['mixTitle', title],
				['mixUrl', url],
				['mixTracklist', file],
				['mixTracklistCid', cid]
			])
		]);

	it('attaches new files, keeps known tracklists, and skips blank rows', async () => {
		const data = rows(
			['Blank', '', empty(), ''],
			['One', 'https://m.test/1', json({ ...sampleReport, extra: 'dropped' }), ''],
			['Two', 'https://m.test/2', empty(), 'bafkept'],
			['Three', 'https://m.test/3', empty(), 'bafunknown']
		);

		const { profile, tracklists } = await withTracklists(data, parseProfile(data), [
			{ ...parseProfile(form(minimal)), mixes: [{ title: 'Old', url: 'https://m.test/old', tracklist: kept }] }
		]);

		expect(profile.mixes?.map((mix) => [mix.title, mix.tracklist?.ref.$link])).toEqual([
			['One', undefined],
			['Two', 'bafkept'],
			['Three', undefined]
		]);
		expect(tracklists).toHaveLength(1);
		expect(tracklists[0]?.mimeType).toBe('application/json');
		const stored = JSON.parse(new TextDecoder().decode(tracklists[0]?.bytes));
		expect(stored.mix.title).toBe(sampleReport.mix.title);
		expect(stored).not.toHaveProperty('extra');
	});
	it('lines tracklists up with the rows kept after dropping repeated links', async () => {
		const data = rows(
			['One', 'https://m.test/1', empty(), ''],
			['One again', 'https://m.test/1', empty(), 'bafkept'],
			['Two', 'https://m.test/2', json(sampleReport), '']
		);

		const { profile, tracklists } = await withTracklists(data, parseProfile(data), [
			{ ...parseProfile(form(minimal)), mixes: [{ title: 'Old', url: 'https://m.test/old', tracklist: kept }] }
		]);

		expect(profile.mixes?.map((mix) => [mix.title, mix.tracklist?.ref.$link])).toEqual([
			['One', undefined],
			['Two', undefined]
		]);
		expect(tracklists.map((upload) => upload?.mimeType)).toEqual([undefined, 'application/json']);
	});
	it('refuses a tracklist for a mix with no link', async () => {
		const data = rows(['One', 'https://m.test/1', empty(), ''], ['No link', '', json(sampleReport), '']);
		await expect(withTracklists(data, parseProfile(data))).rejects.toThrow(/link for the mix/);
	});
	it('refuses a tracklist on a repeated mix row', async () => {
		const data = rows(['One', 'https://m.test/1', empty(), ''], ['One again', 'm.test/1', json(sampleReport), '']);
		await expect(withTracklists(data, parseProfile(data))).rejects.toThrow(/“One again” is listed twice/);
	});
	it('rejects files that are not mix reports', async () => {
		const data = rows(['One', 'https://m.test/1', json({ format: 'something-else' }), '']);
		await expect(withTracklists(data, parseProfile(data))).rejects.toThrow(/tracklist for “One”/);
		const big = rows(['One', 'https://m.test/1', new File([new Uint8Array(1_000_001)], 'mix.json'), '']);
		await expect(withTracklists(big, parseProfile(big))).rejects.toThrow(/over 1 MB/);
	});
});

describe('form round-trips', () => {
	it('turns a profile back into values and echoes raw values', () => {
		const profile = parseProfile(form([...minimal, ['linkKind', 'website'], ['linkUrl', 'https://a.test']]));
		expect(profileToValues(profile, { bookingEmail: 'x@y.test' })).toMatchObject({
			name: 'Neon Tide',
			genres: 'techno, dub techno',
			country: 'DE',
			bookingEmail: 'x@y.test'
		});
		expect(profileToValues(undefined, null)).toEqual({});
		expect(
			profileToValues({ ...profile, fees: [{ label: 'Festivals', tier: 'headliner', minAmount: 2000 }] })
		).toMatchObject({
			fees: [{ label: 'Festivals', tier: 'headliner', currency: '', minAmount: '2000', maxAmount: '' }]
		});
		// A record written before `fees` fills the form's first row.
		expect(profileToValues({ ...profile, fee: { tier: 'emerging', currency: 'GBP' } })).toMatchObject({
			fees: [{ label: '', tier: 'emerging', currency: 'GBP', minAmount: '', maxAmount: '' }]
		});
		expect(
			formValues(
				form([
					...minimal,
					['formats', 'cdj'],
					['formats', 'vinyl'],
					['linkKind', 'website'],
					['linkUrl', 'https://a.test'],
					['mixTitle', 'One'],
					['mixUrl', 'https://m.test'],
					['feeLabel', 'Festivals'],
					['feeTier', ''],
					['feeCurrency', 'eur'],
					['feeMin', '2000'],
					['feeMax', '']
				])
			)
		).toMatchObject({
			formats: ['cdj', 'vinyl'],
			fees: [{ label: 'Festivals', tier: '', currency: 'eur', minAmount: '2000', maxAmount: '' }],
			links: [{ kind: 'website', url: 'https://a.test' }],
			mixes: [{ title: 'One', url: 'https://m.test' }],
			international: false
		});
		expect(formValues(form([...minimal, ['international', 'on']])).international).toBe(true);
	});
	it('echoes link labels, mix lengths and years, and picked sets', () => {
		expect(
			formValues(
				form([
					...minimal,
					['linkKind', 'website'],
					['linkUrl', 'https://a.test'],
					['linkLabel', 'Shop'],
					['mixTitle', 'One'],
					['mixUrl', 'https://m.test'],
					['mixDuration', '90'],
					['mixYear', '2024'],
					['mixSet', 'at://did:plc:neon/computer.localnetwork.dj.set/3kset bafyset']
				])
			)
		).toMatchObject({
			links: [{ kind: 'website', url: 'https://a.test', label: 'Shop' }],
			mixes: [
				{
					title: 'One',
					url: 'https://m.test',
					durationMinutes: 90,
					year: 2024,
					set: { uri: 'at://did:plc:neon/computer.localnetwork.dj.set/3kset', cid: 'bafyset' }
				}
			]
		});
	});
});
