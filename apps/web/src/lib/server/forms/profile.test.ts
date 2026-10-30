import { describe, expect, it } from 'vitest';
import { formValues } from './project';
import { parseProfileForm, profileFormValues, sectionsOf } from './profile';

function form(entries: [string, string][]): FormData {
	const data = new FormData();

	for (const [key, value] of entries) data.append(key, value);

	return data;
}

const basics: [string, string][] = [
	['name', 'Neon'],
	['pronouns', 'they/them'],
	['linkKind', 'soundcloud'],
	['linkUrl', 'soundcloud.com/neon']
];

const performer: [string, string][] = [
	['tagline', 'Hypnotic techno'],
	['genres', 'Techno'],
	['setTypes', 'closing'],
	['feeTier', 'emerging']
];

const promoter: [string, string][] = [
	['bookCity', 'Leipzig'],
	['bookRegion', ''],
	['bookCountry', 'de'],
	['bookCity', ''],
	['bookRegion', ''],
	['bookCountry', '']
];

describe('parseProfileForm', () => {
	it('keeps the basics, a place only when one is given, and only the sections that are on', async () => {
		const input = await parseProfileForm(
			form([...basics, ...performer, ...promoter, ['section', 'promoter']]),
			undefined
		);

		expect(input).toEqual({
			basics: {
				name: 'Neon',
				pronouns: 'they/them',
				links: [{ kind: 'soundcloud', url: 'https://soundcloud.com/neon' }]
			},
			avatar: 'keep',
			promoter: { cities: [{ city: 'Leipzig', country: 'DE' }] }
		});
	});

	it('reads the performer section like a project, with genres optional, and an empty section as on', async () => {
		const input = await parseProfileForm(
			form([...basics, ...performer, ['section', 'performer'], ['city', 'Berlin'], ['country', 'de']]),
			undefined
		);

		expect(input.basics.location).toEqual({ city: 'Berlin', country: 'DE' });
		expect(input.performer).toEqual({
			tagline: 'Hypnotic techno',
			genres: ['techno'],
			setTypes: ['closing'],
			fees: [{ tier: 'emerging' }]
		});
		expect(
			(
				await parseProfileForm(
					form([
						['name', 'Neon'],
						['section', 'performer']
					]),
					undefined
				)
			).performer
		).toEqual({});
	});

	it('says what to fix', async () => {
		await expect(parseProfileForm(form([['name', '']]), undefined)).rejects.toThrow('Add your name');
		await expect(
			parseProfileForm(
				form([
					['name', 'Neon'],
					['country', 'DE']
				]),
				undefined
			)
		).rejects.toThrow("Add the city you're based in, or clear the place");
		await expect(
			parseProfileForm(
				form([
					['name', 'Neon'],
					['section', 'promoter'],
					['bookCity', 'Leipzig'],
					['bookCountry', 'Saxony']
				]),
				undefined
			)
		).rejects.toThrow('Use a two-letter country code for Leipzig');
		await expect(
			parseProfileForm(
				form([
					['name', 'Neon'],
					['section', 'promoter'],
					['bookCountry', 'DE']
				]),
				undefined
			)
		).rejects.toThrow('Name each city you book in, or clear its row');
	});

	it('removes the avatar only when asked, and lists each city once', async () => {
		const input = await parseProfileForm(
			form([
				['name', 'Neon'],
				['removeAvatar', 'on'],
				['section', 'promoter'],
				['bookCity', 'Leipzig'],
				['bookCountry', 'DE'],
				['bookCity', 'leipzig'],
				['bookCountry', 'DE']
			]),
			undefined
		);

		expect(input.avatar).toBe('remove');
		expect(input.promoter).toEqual({ cities: [{ city: 'Leipzig', country: 'DE' }] });
	});
});

describe('profileFormValues and sectionsOf', () => {
	it('fill the form back in from a saved profile, and say which sections are on', () => {
		const record = {
			basics: { name: 'Neon', location: { city: 'Berlin', country: 'DE' } },
			performer: { genres: ['techno', 'dub techno'], highlights: ['Tresor', 'Panorama Bar'] },
			promoter: { cities: [{ city: 'Leipzig', region: 'Saxony', country: 'DE' }] },
			createdAt: '2026-10-30T00:00:00.000Z',
			updatedAt: '2026-10-30T00:00:00.000Z'
		};

		expect(profileFormValues(record)).toMatchObject({
			name: 'Neon',
			city: 'Berlin',
			genres: 'techno, dub techno',
			highlights: 'Tresor\nPanorama Bar',
			cities: [{ city: 'Leipzig', region: 'Saxony', country: 'DE' }]
		});
		expect(sectionsOf(record)).toEqual({ performer: true, promoter: true });
		expect(sectionsOf({ ...record, performer: undefined, promoter: undefined })).toEqual({
			performer: false,
			promoter: false
		});
	});

	it('echo the city rows back after an error', () => {
		expect(formValues(form(promoter)).cities).toEqual([
			{ city: 'Leipzig', region: '', country: 'de' },
			{ city: '', region: '', country: '' }
		]);
	});
});
