import { deflateSync } from 'node:zlib';
import * as fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import type { EpkPhoto } from '#lib/epk.ts';
import type { ProjectMix } from '#lib/types.ts';
import { UserError } from '../core';
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
		await expect(
			parseEpkForm(
				form([
					['title', 'x'],
					['newPhotos', new File(['GIF89a'], 'x.gif', { type: 'image/gif' })]
				]),
				[],
				[]
			)
		).rejects.toThrow("x.gif isn't a PNG, JPEG or WebP image");
		await expect(
			parseEpkForm(
				form([
					['title', 'x'],
					['newPhotos', new File([new Uint8Array(5_000_001)], 'big.png', { type: 'image/png' })]
				]),
				[],
				[]
			)
		).rejects.toThrow('big.png is over 5 MB');
	});

	it('uploads a photo whose size it can’t read without one', async () => {
		const signatureOnly = new File([(await png(1, 1).bytes()).subarray(0, 8)], 'cut.png', { type: 'image/png' });

		const parsed = await parseEpkForm(
			form([
				['title', 'x'],
				['newPhotos', png(0, 0)],
				['newPhotos', signatureOnly]
			]),
			[],
			[]
		);

		expect(parsed.uploads.map((upload) => [upload.visibility, upload.aspectRatio, upload.credit])).toEqual([
			['public', undefined, undefined],
			['public', undefined, undefined]
		]);
	});

	it('leaves out mixes the form says nothing about, hidden extras, and extras the project already has', async () => {
		const parsed = await parseEpkForm(
			form([
				['title', 'Clubs'],
				[`mixVisibility.${projectMixes[1].url}`, 'private'],
				['extraMixTitle', 'Promo'],
				['extraMixUrl', 'https://soundcloud.com/neon/promo'],
				['extraMixVisibility.0', 'public'],
				['extraMixTitle', 'Secret'],
				['extraMixUrl', 'https://soundcloud.com/neon/secret'],
				['extraMixVisibility.1', 'hidden'],
				['extraMixTitle', 'Again'],
				['extraMixUrl', projectMixes[0].url],
				['extraMixVisibility.2', 'public'],
				['extraMixTitle', ''],
				['extraMixUrl', ''],
				['extraMixTitle', 'Last'],
				['extraMixUrl', 'https://soundcloud.com/neon/last'],
				['extraMixVisibility.4', 'private']
			]),
			projectMixes,
			[]
		);

		expect(parsed.publicMixes).toEqual([{ title: 'Promo', url: 'https://soundcloud.com/neon/promo' }]);
		expect(parsed.privateMixes.map((mix) => mix.title)).toEqual(['Late show', 'Last']);
		await expect(
			parseEpkForm(
				form([
					['title', 'x'],
					['extraMixTitle', 'Promo']
				]),
				[],
				[]
			)
		).rejects.toThrow('Give each extra mix a name and a web address');
	});

	it('shows at most 12 mixes to each audience', async () => {
		const extras = (count: number, place: string) =>
			Array.from({ length: count }, (_, index): [string, string][] => [
				['extraMixTitle', `Mix ${index}`],
				['extraMixUrl', `https://m.test/${index}`],
				[`extraMixVisibility.${index}`, place]
			]).flat();

		for (const place of ['public', 'private']) {
			await expect(parseEpkForm(form([['title', 'x'], ...extras(13, place)]), [], [])).rejects.toThrow(
				/at most 12 mixes/
			);
			expect((await parseEpkForm(form([['title', 'x'], ...extras(12, place)]), [], [])).cid).toBe('');
		}
	});

	it('keeps a photo’s own alt text when the form’s is empty, and drops a credit that was cleared', async () => {
		const parsed = await parseEpkForm(
			form([
				['title', 'Clubs'],
				['photoCid', 'bafyone'],
				['photoAlt', new File(['not text'], 'alt.txt')],
				['photoCredit', ''],
				['photoVisibility.0', 'public'],
				['photoCid', 'bafytwo'],
				['photoVisibility.1', 'hidden']
			]),
			[],
			[{ ...photo('bafyone'), credit: 'Old credit' }, photo('bafytwo')]
		);

		expect(parsed.publicPhotos).toEqual([photo('bafyone')]);
		expect(parsed.privatePhotos).toEqual([]);
	});

	it('holds at most 10 photos for everyone and 20 for people with access', async () => {
		const held = Array.from({ length: 21 }, (_, index) => photo(`bafy${index}`));

		const photos = (count: number, place: string) =>
			held.slice(0, count).flatMap((entry, index): [string, string][] => [
				['photoCid', entry.image.ref.$link],
				[`photoVisibility.${index}`, place]
			]);

		const parse = (count: number, place: string) =>
			parseEpkForm(form([['title', 'x'], ...photos(count, place)]), [], held);

		await expect(parse(11, 'public')).rejects.toThrow('at most 10 photos to everyone');
		await expect(parse(21, 'private')).rejects.toThrow('at most 20 photos for people with access');
		expect((await parse(10, 'public')).publicPhotos).toHaveLength(10);
		expect((await parse(20, 'private')).privatePhotos).toHaveLength(20);
	});

	it('keeps a quote’s link and the tech rider, and refuses a link that isn’t one', async () => {
		const parsed = await parseEpkForm(
			form([
				['title', 'Clubs'],
				['quoteText', 'Best set all year'],
				['quoteSource', 'Halle Nord'],
				['quoteUrl', 'https://halle.test/review'],
				['techRider', 'Two CDJ-3000s']
			]),
			[],
			[]
		);

		expect(parsed.content).toEqual({
			quotes: [{ text: 'Best set all year', source: 'Halle Nord', url: 'https://halle.test/review' }],
			techRider: 'Two CDJ-3000s'
		});
		await expect(
			parseEpkForm(
				form([
					['title', 'x'],
					['quoteText', 'Loved it'],
					['quoteSource', 'Halle Nord'],
					['quoteUrl', 'halle']
				]),
				[],
				[]
			)
		).rejects.toThrow("“halle” isn't a web address");
		await expect(
			parseEpkForm(
				form([
					['title', 'x'],
					['quoteText', 'Loved it']
				]),
				[],
				[]
			)
		).rejects.toThrow(/who said it/);
	});
});

describe('parseEpkForm on any submission', () => {
	const field = fc.constantFrom(
		'cid',
		'title',
		'preset',
		`mixVisibility.${projectMixes[0].url}`,
		'extraMixTitle',
		'extraMixUrl',
		'extraMixVisibility.0',
		'photoCid',
		'photoAlt',
		'photoCredit',
		'photoVisibility.0',
		'quoteText',
		'quoteSource',
		'quoteUrl',
		'email',
		'techRider'
	);

	const value = fc.oneof(fc.string(), fc.constantFrom('', 'public', 'private', 'hidden', 'bafyone', 'https://x.test'));

	it('either reads it or says what to fix, and can always give it back to the form', async () => {
		await fc.assert(
			fc.asyncProperty(fc.array(fc.tuple(field, value), { maxLength: 30 }), async (entries) => {
				const data = form(entries);

				// A title, since a press kit can't be saved without one, or an error saying what to fix.
				const readOrExplained = await parseEpkForm(data, projectMixes, [photo('bafyone')]).then(
					(parsed) => parsed.title !== '',
					(error: Error) => error instanceof UserError
				);

				expect(readOrExplained).toBe(true);
				expect(submittedValues(data, projectMixes).mixes).toHaveLength(projectMixes.length);
			})
		);
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
				title: 'Clubs',
				access: { bookedWith: 'auto', following: 'none', promoters: 'none', anyone: 'none' },
				mixes: [{ title: 'Tidal Hours 004', url: projectMixes[0].url }],
				photos: [photo('bafyone')],
				content: { mixes: [{ title: 'Promo', url: 'https://soundcloud.com/neon/promo' }] }
			},
			VERSION
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

	it('gives back each photo row, with empty text for fields a row is missing', () => {
		const values = submittedValues(
			form([
				['photoCid', 'bafyone'],
				['photoAlt', 'At Halle Nord'],
				['photoCredit', 'Mara Okafor'],
				['photoVisibility.0', 'private'],
				['photoCid', 'bafytwo'],
				['extraMixTitle', ''],
				['extraMixUrl', '']
			]),
			[]
		);

		expect(values.photos).toEqual([
			{ cid: 'bafyone', alt: 'At Halle Nord', credit: 'Mara Okafor', visibility: 'private' },
			{ cid: 'bafytwo', alt: '', credit: '', visibility: 'public' }
		]);
		expect(values.extraMixes).toEqual([]);
	});

	it('shows project mixes and photos kept for people with access as private', () => {
		const values = epkFormValues(projectMixes, {
			title: 'Clubs',
			access: { bookedWith: 'auto', following: 'none', promoters: 'none', anyone: 'none' },
			content: {
				mixes: [{ title: 'Late show', url: projectMixes[1].url }],
				photos: [{ ...photo('bafyone'), credit: 'Mara Okafor' }]
			}
		});

		expect(values.mixes.map((mix) => mix.visibility)).toEqual(['hidden', 'private', 'hidden']);
		expect(values.extraMixes).toEqual([]);
		expect(values.photos).toEqual([
			{
				cid: 'bafyone',
				alt: 'Neon Tide, press photo',
				credit: 'Mara Okafor',
				visibility: 'private',
				width: 3200,
				height: 1800
			}
		]);
	});
});
