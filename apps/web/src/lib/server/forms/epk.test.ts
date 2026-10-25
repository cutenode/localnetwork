import { deflateSync } from 'node:zlib';
import { describe, expect, it } from 'vitest';
import type { EpkPhoto } from '#lib/epk.ts';
import type { ProjectMix } from '#lib/types.ts';
import { epkFormValues, parseEpkForm, submittedValues } from './epk';

const tracklist = { $type: 'blob' as const, ref: { $link: 'bafytracks' }, mimeType: 'application/json', size: 10 };

const projectMixes: ProjectMix[] = [
	{ title: 'Tidal Hours 004', url: 'https://soundcloud.com/neon/004', tracklist },
	{ title: 'Late show', url: 'https://mixcloud.com/neon/late' },
	{ title: 'Old one', url: 'https://soundcloud.com/neon/old' }
];

const photo = (cid: string): EpkPhoto => ({
	image: { $type: 'blob', ref: { $link: cid }, mimeType: 'image/jpeg', size: 1000 },
	alt: 'Neon Tide, press photo',
	aspectRatio: { width: 3200, height: 1800 }
});

/** A PNG header with the given size: enough for type sniffing and reading dimensions. */
function png(width: number, height: number): File {
	const ihdr = Buffer.alloc(25);
	ihdr.writeUInt32BE(13, 0);
	ihdr.write('IHDR', 4);
	ihdr.writeUInt32BE(width, 8);
	ihdr.writeUInt32BE(height, 12);
	ihdr[16] = 8;
	ihdr[17] = 2;
	const bytes = Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), ihdr, deflateSync(Buffer.alloc(8))]);

	return new File([bytes], 'shot.png', { type: 'image/png' });
}

const form = (entries: [string, string | File][]) => {
	const data = new FormData();

	for (const [name, value] of entries) data.append(name, value);

	return data;
};

/** A record version, as the edit form carries it. */
const VERSION = 'bafyreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku';

describe('parseEpkForm', () => {
	it('reads the version the form was opened on, which only a new press kit leaves empty', async () => {
		const parse = (cid: string) =>
			parseEpkForm(
				form([
					['title', 'Clubs'],
					['cid', cid]
				]),
				[],
				[]
			);

		expect((await parse(VERSION)).cid).toBe(VERSION);
		expect((await parse('')).cid).toBe('');
		await expect(parse('not-a-cid')).rejects.toThrow('Reload the press kit and try again');
	});

	it('splits mixes and photos by who sees them, taking blobs only from what the press kit holds', async () => {
		const parsed = await parseEpkForm(
			form([
				['title', 'Clubs'],
				['preset', 'open'],
				[`mixVisibility.${projectMixes[0].url}`, 'public'],
				[`mixVisibility.${projectMixes[1].url}`, 'private'],
				[`mixVisibility.${projectMixes[2].url}`, 'hidden'],
				['extraMixTitle', 'Promo'],
				['extraMixUrl', 'https://soundcloud.com/neon/promo'],
				['extraMixVisibility.0', 'private'],
				['photoCid', 'bafyone'],
				['photoAlt', 'Neon Tide at Halle Nord'],
				['photoCredit', 'Mara Okafor'],
				['photoVisibility.0', 'private'],
				['photoCid', 'bafyforged'],
				['photoVisibility.1', 'public']
			]),
			projectMixes,
			[photo('bafyone')]
		);

		expect(parsed.publicMixes).toEqual([{ title: 'Tidal Hours 004', url: 'https://soundcloud.com/neon/004' }]);
		expect(parsed.privateMixes.map((mix) => mix.title)).toEqual(['Late show', 'Promo']);
		expect(parsed.publicPhotos).toEqual([]);
		expect(parsed.privatePhotos).toEqual([
			{ ...photo('bafyone'), alt: 'Neon Tide at Halle Nord', credit: 'Mara Okafor' }
		]);
		expect(parsed.access.anyone).toBe('auto');
	});

	it('reads its own mix of rules when no preset is picked', async () => {
		const parsed = await parseEpkForm(
			form([
				['title', 'Clubs'],
				['preset', 'custom'],
				['access.bookedWith', 'auto'],
				['access.following', 'review'],
				['access.promoters', 'whatever'],
				['access.anyone', 'none']
			]),
			[],
			[]
		);

		expect(parsed.access).toEqual({ bookedWith: 'auto', following: 'review', promoters: 'none', anyone: 'none' });
	});

	it('keeps the private part to what was filled in', async () => {
		const parsed = await parseEpkForm(
			form([
				['title', 'Clubs'],
				['longBio', 'The long one'],
				['quoteText', 'Best set all year'],
				['quoteSource', 'Halle Nord'],
				['quoteUrl', ''],
				['quoteText', ''],
				['quoteSource', ''],
				['quoteUrl', ''],
				['email', 'book@neon.test'],
				['phone', '']
			]),
			[],
			[]
		);

		expect(parsed.content).toEqual({
			bio: 'The long one',
			quotes: [{ text: 'Best set all year', source: 'Halle Nord' }],
			contact: { email: 'book@neon.test' }
		});
	});

	it('reads new photos with their size, and refuses what press kits can’t hold', async () => {
		const parsed = await parseEpkForm(
			form([
				['title', 'Clubs'],
				['newPhotos', png(3200, 1800)],
				['newPhotoVisibility', 'private'],
				['newPhotoCredit', 'Mara Okafor']
			]),
			[],
			[]
		);

		expect(parsed.uploads).toMatchObject([
			{
				mimeType: 'image/png',
				aspectRatio: { width: 3200, height: 1800 },
				visibility: 'private',
				credit: 'Mara Okafor'
			}
		]);
		await expect(parseEpkForm(form([['title', '']]), [], [])).rejects.toThrow(/name/);
		await expect(
			parseEpkForm(
				form([
					['title', 'x'],
					['email', 'nope']
				]),
				[],
				[]
			)
		).rejects.toThrow(/email/);
		await expect(
			parseEpkForm(
				form([
					['title', 'x'],
					['quoteText', 'Loved it'],
					['quoteSource', '']
				]),
				[],
				[]
			)
		).rejects.toThrow(/who said it/);
		await expect(
			parseEpkForm(
				form([
					['title', 'x'],
					['extraMixTitle', 'Promo'],
					['extraMixUrl', 'not a url']
				]),
				[],
				[]
			)
		).rejects.toThrow(/web address/);
		await expect(
			parseEpkForm(
				form([
					['title', 'x'],
					['newPhotos', new File(['hi'], 'x.png', { type: 'image/png' })]
				]),
				[],
				[]
			)
		).rejects.toThrow(/isn't the image type/);
	});
});

describe('form values', () => {
	it('starts a new press kit with every project mix public and the default rules', () => {
		const values = epkFormValues(projectMixes);

		expect(values.mixes.map((mix) => mix.visibility)).toEqual(['public', 'public', 'public']);
		expect(values.preset).toBe('review');
	});

	it('shows where each saved mix and photo is, and keeps mixes the project no longer has', () => {
		const values = epkFormValues(
			projectMixes,
			{
				cid: VERSION,
				record: {
					project: 'at://x',
					title: 'Clubs',
					access: { bookedWith: 'auto', following: 'none', promoters: 'none', anyone: 'none' },
					mixes: [{ title: 'Tidal Hours 004', url: projectMixes[0].url }],
					photos: [photo('bafyone')],
					createdAt: '2026-10-01T00:00:00Z'
				}
			},
			{ mixes: [{ title: 'Promo', url: 'https://soundcloud.com/neon/promo' }], updatedAt: '2026-10-01T00:00:00Z' }
		);

		expect(values.mixes.map((mix) => mix.visibility)).toEqual(['public', 'hidden', 'hidden']);
		expect(values.extraMixes).toEqual([
			{ title: 'Promo', url: 'https://soundcloud.com/neon/promo', visibility: 'private' }
		]);
		expect(values.photos).toEqual([
			{ cid: 'bafyone', alt: 'Neon Tide, press photo', credit: '', visibility: 'public', width: 3200, height: 1800 }
		]);
		expect(values.preset).toBe('custom');
		expect(values.cid).toBe(VERSION);
		expect(epkFormValues(projectMixes).cid).toBe('');
	});

	it('gives a failed submission back as it was typed', () => {
		const values = submittedValues(
			form([
				['title', 'Clubs'],
				[`mixVisibility.${projectMixes[1].url}`, 'private'],
				['extraMixTitle', 'Promo'],
				['extraMixUrl', 'bad'],
				['extraMixVisibility.0', 'public'],
				['cid', VERSION]
			]),
			projectMixes
		);

		expect(values.cid).toBe(VERSION);

		expect(values.mixes[1].visibility).toBe('private');
		expect(values.extraMixes).toEqual([{ title: 'Promo', url: 'bad', visibility: 'public' }]);
	});
});
