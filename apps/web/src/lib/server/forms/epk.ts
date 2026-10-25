import { imageSize } from 'image-size';
import {
	ACCESS_PRESETS,
	DEFAULT_ACCESS,
	type EpkAccess,
	type EpkContent,
	type EpkMix,
	type EpkPhoto,
	type EpkQuote,
	type EpkFormPhoto,
	type EpkFormValues,
	type EpkRecord,
	presetFor,
	readRule,
	type Visibility
} from '#lib/epk.ts';
import type { ProjectMix } from '#lib/types.ts';
import { UserError } from '../core';
import { isWebUrl, text } from '../http';
import { CID, sniffImage } from './project';

/** Big enough for a 3,000 px JPEG, the size press asks for, and under the 5 MiB some PDSes allow. */
export const EPK_PHOTO_MAX_BYTES = 5_000_000;

const PHOTO_TYPES = new Set(['image/png', 'image/jpeg', 'image/webp']);

const QUOTE_ROWS = 5;

export type PhotoUpload = {
	bytes: Uint8Array;
	mimeType: string;
	aspectRatio?: { width: number; height: number };
	visibility: 'public' | 'private';
	credit?: string;
};

const visibility = (value: string): Visibility => (value === 'private' || value === 'hidden' ? value : 'public');

/** Every value of a repeated field; a file in its place reads as empty. */
const all = (form: FormData, name: string) =>
	form.getAll(name).map((value) => (value instanceof File ? '' : value.trim()));

/** The form's starting values: a new press kit shows every project mix publicly. */
export function epkFormValues(
	projectMixes: ProjectMix[],
	epk?: { record: EpkRecord; cid: string },
	content?: EpkContent | null
): EpkFormValues {
	const record = epk?.record;

	const where = (url: string): Visibility => {
		if (!record) return 'public';

		if (record.mixes?.some((mix) => mix.url === url)) return 'public';

		return content?.mixes?.some((mix) => mix.url === url) ? 'private' : 'hidden';
	};

	const access = record?.access ?? DEFAULT_ACCESS;
	const projectUrls = new Set(projectMixes.map((mix) => mix.url));

	const extra = (mixes: EpkMix[] | undefined, place: Visibility) =>
		(mixes ?? [])
			.filter((mix) => !projectUrls.has(mix.url))
			.map((mix) => ({ title: mix.title, url: mix.url, visibility: place }));

	const photo = (entry: EpkPhoto, place: Visibility): EpkFormPhoto => ({
		cid: entry.image.ref.$link,
		alt: entry.alt,
		credit: entry.credit ?? '',
		visibility: place,
		...entry.aspectRatio
	});

	return {
		cid: epk?.cid ?? '',
		title: record?.title ?? '',
		headline: record?.headline ?? '',
		bio: record?.bio ?? '',
		longBio: content?.bio ?? '',
		preset: presetFor(access)?.id ?? 'custom',
		access,
		mixes: projectMixes.map((mix) => ({ title: mix.title, url: mix.url, visibility: where(mix.url) })),
		extraMixes: [...extra(record?.mixes, 'public'), ...extra(content?.mixes, 'private')],
		photos: [
			...(record?.photos ?? []).map((entry) => photo(entry, 'public')),
			...(content?.photos ?? []).map((entry) => photo(entry, 'private'))
		],
		quotes: content?.quotes ?? [],
		email: content?.contact?.email ?? '',
		phone: content?.contact?.phone ?? '',
		contactNote: content?.contact?.note ?? '',
		techRider: content?.techRider ?? ''
	};
}

/** A submission's values, for showing the form again after an error. Files are left out. */
export function submittedValues(form: FormData, projectMixes: ProjectMix[]): EpkFormValues {
	const cids = all(form, 'photoCid');
	const preset = text(form, 'preset', 32);

	return {
		cid: text(form, 'cid', 128),
		title: text(form, 'title', 100),
		headline: text(form, 'headline', 300),
		bio: text(form, 'bio', 1000),
		longBio: text(form, 'longBio', 5000),
		preset,
		access: parseAccess(form),
		mixes: projectMixes.map((mix) => ({
			title: mix.title,
			url: mix.url,
			visibility: visibility(text(form, `mixVisibility.${mix.url}`, 16))
		})),
		extraMixes: parseExtraMixes(form, false),
		photos: cids.map((cid, index) => ({
			cid,
			alt: all(form, 'photoAlt')[index] ?? '',
			credit: all(form, 'photoCredit')[index] ?? '',
			visibility: visibility(text(form, `photoVisibility.${index}`, 16))
		})),
		quotes: parseQuotes(form, false),
		email: text(form, 'email', 320),
		phone: text(form, 'phone', 64),
		contactNote: text(form, 'contactNote', 500),
		techRider: text(form, 'techRider', 2000)
	};
}

/** The access rules: a preset, or each audience's rule when the owner picked their own. */
export function parseAccess(form: FormData): EpkAccess {
	const preset = ACCESS_PRESETS.find((entry) => entry.id === text(form, 'preset', 32));

	if (preset) return preset.access;

	return {
		bookedWith: readRule(text(form, 'access.bookedWith', 16)),
		following: readRule(text(form, 'access.following', 16)),
		promoters: readRule(text(form, 'access.promoters', 16)),
		anyone: readRule(text(form, 'access.anyone', 16))
	};
}

/** Rows of mixes that aren't on the project; a row with neither a title nor an address is skipped. */
function parseExtraMixes(form: FormData, strict: boolean): EpkFormValues['extraMixes'] {
	const titles = all(form, 'extraMixTitle');
	const urls = all(form, 'extraMixUrl');
	const mixes: EpkFormValues['extraMixes'] = [];

	for (const [index, title] of titles.entries()) {
		const url = urls[index] ?? '';

		if (!title && !url) continue;

		if (strict && (!title || !isWebUrl(url))) throw new UserError('Give each extra mix a name and a web address');
		mixes.push({
			title: title.slice(0, 120),
			url,
			visibility: visibility(text(form, `extraMixVisibility.${index}`, 16))
		});
	}

	return mixes;
}

function parseQuotes(form: FormData, strict: boolean): EpkQuote[] {
	const texts = all(form, 'quoteText');
	const sources = all(form, 'quoteSource');
	const urls = all(form, 'quoteUrl');
	const quotes: EpkQuote[] = [];

	for (let index = 0; index < Math.min(texts.length, QUOTE_ROWS); index++) {
		const quote = texts[index].slice(0, 500);
		const source = (sources[index] ?? '').slice(0, 200);
		const url = urls[index] ?? '';

		if (!quote && !source) continue;

		if (strict && (!quote || !source)) throw new UserError('Give each press quote its words and who said it');

		if (strict && url && !isWebUrl(url)) throw new UserError(`“${url}” isn't a web address`);

		quotes.push(url ? { text: quote, source, url } : { text: quote, source });
	}

	return quotes;
}

/**
 * Reads the press kit form. Mixes come from the project and photos from what the press kit already
 * holds, looked up by CID; the form only says where each one shows.
 */
export async function parseEpkForm(
	form: FormData,
	projectMixes: ProjectMix[],
	existingPhotos: EpkPhoto[]
): Promise<{
	cid: string;
	title: string;
	headline?: string;
	bio?: string;
	access: EpkAccess;
	publicMixes: EpkMix[];
	privateMixes: EpkMix[];
	publicPhotos: EpkPhoto[];
	privatePhotos: EpkPhoto[];
	uploads: PhotoUpload[];
	content: Omit<EpkContent, 'updatedAt' | 'mixes' | 'photos'>;
}> {
	const cid = text(form, 'cid', 128);

	if (cid && !CID.test(cid)) throw new UserError('Reload the press kit and try again');
	const title = text(form, 'title', 100);

	if (!title) throw new UserError('Give the press kit a name, like “Clubs” or “Festivals”');
	const publicMixes: EpkMix[] = [];
	const privateMixes: EpkMix[] = [];

	for (const mix of projectMixes) {
		// A mix added to the project since the form loaded isn't on it, so it stays out.
		const place = visibility(text(form, `mixVisibility.${mix.url}`, 16) || 'hidden');
		const { tracklist: _tracklist, ...entry } = mix;

		if (place === 'public') publicMixes.push(entry);
		else if (place === 'private') privateMixes.push(entry);
	}

	for (const mix of parseExtraMixes(form, true)) {
		if (mix.visibility === 'hidden' || projectMixes.some((entry) => entry.url === mix.url)) continue;
		(mix.visibility === 'public' ? publicMixes : privateMixes).push({ title: mix.title, url: mix.url });
	}

	if (publicMixes.length > 12 || privateMixes.length > 12)
		throw new UserError('A press kit can show at most 12 mixes to everyone and 12 on request');

	const byCid = new Map(existingPhotos.map((photo) => [photo.image.ref.$link, photo]));
	const alts = all(form, 'photoAlt');
	const credits = all(form, 'photoCredit');
	const publicPhotos: EpkPhoto[] = [];
	const privatePhotos: EpkPhoto[] = [];

	for (const [index, photoCid] of all(form, 'photoCid').entries()) {
		const found = byCid.get(photoCid);
		const place = visibility(text(form, `photoVisibility.${index}`, 16));

		if (!found || place === 'hidden') continue;
		const credit = credits[index]?.slice(0, 200);
		const photo: EpkPhoto = { ...found, alt: (alts[index] || found.alt).slice(0, 1000) };

		if (credit) photo.credit = credit;
		else delete photo.credit;

		(place === 'public' ? publicPhotos : privatePhotos).push(photo);
	}

	const uploads = await parseUploads(form);

	if (publicPhotos.length + uploads.filter((upload) => upload.visibility === 'public').length > 10)
		throw new UserError('A press kit can show at most 10 photos to everyone');

	if (privatePhotos.length + uploads.filter((upload) => upload.visibility === 'private').length > 20)
		throw new UserError('A press kit can hold at most 20 photos for people with access');

	const email = text(form, 'email', 320);

	if (email && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) throw new UserError(`“${email}” isn't an email address`);
	const contact = { email, phone: text(form, 'phone', 64), note: text(form, 'contactNote', 500) };
	const content: Omit<EpkContent, 'updatedAt' | 'mixes' | 'photos'> = {};
	const longBio = text(form, 'longBio', 5000);
	const techRider = text(form, 'techRider', 2000);
	const quotes = parseQuotes(form, true);

	if (longBio) content.bio = longBio;

	if (quotes.length) content.quotes = quotes;

	if (contact.email || contact.phone || contact.note)
		content.contact = Object.fromEntries(Object.entries(contact).filter(([, value]) => value));

	if (techRider) content.techRider = techRider;

	return {
		cid,
		title,
		headline: text(form, 'headline', 300) || undefined,
		bio: text(form, 'bio', 1000) || undefined,
		access: parseAccess(form),
		publicMixes,
		privateMixes,
		publicPhotos,
		privatePhotos,
		uploads,
		content
	};
}

async function parseUploads(form: FormData): Promise<PhotoUpload[]> {
	const files = form.getAll('newPhotos').filter((file) => file instanceof File && file.size > 0);
	const place = text(form, 'newPhotoVisibility', 16) === 'private' ? 'private' : 'public';
	const credit = text(form, 'newPhotoCredit', 200) || undefined;
	const uploads: PhotoUpload[] = [];

	for (const file of files) {
		if (!(file instanceof File)) continue;

		if (!PHOTO_TYPES.has(file.type)) throw new UserError(`${file.name} isn't a PNG, JPEG or WebP image`);

		if (file.size > EPK_PHOTO_MAX_BYTES) throw new UserError(`${file.name} is over 5 MB`);
		const bytes = new Uint8Array(await file.arrayBuffer());

		if (!sniffImage(bytes, file.type)) throw new UserError(`${file.name} isn't the image type it claims to be`);
		uploads.push({ bytes, mimeType: file.type, aspectRatio: dimensions(bytes), visibility: place, credit });
	}

	return uploads;
}

/** A photo's size in pixels, which the press kit shows against what press asks for. */
function dimensions(bytes: Uint8Array): { width: number; height: number } | undefined {
	try {
		const { width, height } = imageSize(bytes);

		return width && height ? { width, height } : undefined;
	} catch {
		return undefined;
	}
}
