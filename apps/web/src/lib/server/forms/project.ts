import { MIX_REPORT_MAX_BYTES } from '#lib/mix/report.ts';
import { parseMixReportText } from '#lib/mix/schema.ts';
import { COLLECTION } from '#lib/nsid.ts';
import { normalizeGenres } from '#lib/taxonomy/genres.ts';
import { FEE_TIERS, FORMATS, isOneOf, LINK_KINDS, PROJECT_KINDS, SET_TYPES } from '#lib/taxonomy/options.ts';
import { feeLevels } from '#lib/format.ts';
import type {
	BasicsSection,
	BlobRefJson,
	FeeFormRow,
	Location,
	PerformerSection,
	PromoterSection,
	ProjectDetails,
	ProjectFee,
	ProjectFormValues,
	ProjectLink,
	ProjectMix,
	ProjectProfile,
	StrongRef
} from '#lib/types.ts';
import { parsePublicRecordUri } from '../atproto/spaces';
import { UserError } from '../core';
import { all, CID, currencyCode, limit, optionalInt, optionalText, text, wholeNumber } from '../http';
import { AVATAR_TYPES } from '../images';

const AVATAR_MAX_BYTES = 1_000_000;

function toUrl(value: string, field: string): string {
	let parsed: URL;

	try {
		parsed = new URL(value.includes('://') ? value : `https://${value}`);
	} catch {
		throw new UserError(`${field} is not a valid link`);
	}

	if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') throw new UserError(`${field} must be a web link`);

	return parsed.toString();
}

const strings = (form: FormData, field: string) =>
	form.getAll(field).map((value) => (value instanceof File ? '' : value));

/** Each distinct link in a repeated URL field, mapped to the form row it first appears in. */
function linkRows(form: FormData, field: string, label: string): Map<string, number> {
	const rows = new Map<string, number>();
	strings(form, field).forEach((raw, row) => {
		const value = raw.trim();

		if (!value) return;
		const url = toUrl(value, label);

		if (!rows.has(url)) rows.set(url, row);
	});

	return rows;
}

/**
 * A mix's set picker value: the set's URI and the CID of the version picked, space-separated (see
 * ProjectForm.svelte), or nothing. Whether the set exists and whose it is gets checked when root publishes.
 */
function setRef(value: string): StrongRef | undefined {
	if (!value) return undefined;
	const [uri = '', cid = ''] = value.split(' ');

	if (parsePublicRecordUri(uri)?.collection !== COLLECTION.djSet || !CID.test(cid))
		throw new UserError('Pick a set from the list');

	return { uri, cid };
}

/** The project form's fee rows, trimmed. Each row posts one of each field. */
function feeRows(form: FormData): FeeFormRow[] {
	const fields = ['feeLabel', 'feeTier', 'feeCurrency', 'feeMin', 'feeMax'].map((field) =>
		strings(form, field).map((value) => value.trim())
	);

	const [labels, tiers, currencies, mins, maxes] = fields;

	return Array.from({ length: Math.max(...fields.map((values) => values.length)) }, (_, row) => ({
		label: labels[row] ?? '',
		tier: tiers[row] ?? '',
		currency: currencies[row] ?? '',
		minAmount: mins[row] ?? '',
		maxAmount: maxes[row] ?? ''
	}));
}

/**
 * Where someone is, from the place picker's fields: required for a project, optional on a profile (left out when
 * every field is empty). `missing` is the error for a place without a city.
 */
export function parseLocation(form: FormData, required: boolean, missing: string): Location | undefined {
	const city = limit('City', text(form, 'city', 1000), 100);
	const country = text(form, 'country', 8).toUpperCase();
	const region = limit('Region', optionalText(form, 'region', 1000), 100);

	if (!required && !city && !country && !region) return undefined;

	if (!city) throw new UserError(missing);

	if (!/^[A-Z]{2}$/.test(country)) throw new UserError('Use a two-letter country code (like US, GB, or DE)');
	const latE6 = optionalInt(form, 'latE6', -90_000_000, 90_000_000);
	const lngE6 = optionalInt(form, 'lngE6', -180_000_000, 180_000_000);
	const location: Location = { city, country };

	if (region) location.region = region;

	if (latE6 !== undefined && lngE6 !== undefined) {
		location.latE6 = latE6;
		location.lngE6 = lngE6;
	}

	return location;
}

/**
 * The basics every profile and project has (`computer.localnetwork.section.defs#basics`), from BasicsFields: name,
 * pronouns, bio and links. The place and the avatar are parsed on their own (parseLocation, parseAvatar), since
 * projects and profiles treat them differently. `unnamed` is the error for a missing name.
 */
export function parseBasics(form: FormData, unnamed: string): Omit<BasicsSection, 'location' | 'avatar'> {
	const name = limit('Name', text(form, 'name', 640), 64);

	if (!name) throw new UserError(unnamed);
	const basics: Omit<BasicsSection, 'location' | 'avatar'> = { name };
	const bio = limit('Bio', optionalText(form, 'bio', 30_000), 3000);

	if (bio) basics.bio = bio;
	const pronouns = limit('Pronouns', optionalText(form, 'pronouns', 200), 24);

	if (pronouns) basics.pronouns = pronouns;
	const linkKinds = strings(form, 'linkKind');
	const linkLabels = strings(form, 'linkLabel');

	const links: ProjectLink[] = [...linkRows(form, 'linkUrl', 'A link')].map(([url, row]) => {
		const linkKind = linkKinds[row] || 'other';
		const label = limit('A link label', (linkLabels[row] ?? '').trim() || undefined, 64);
		const link: ProjectLink = { kind: isOneOf(LINK_KINDS, linkKind) ? linkKind : 'other', url };

		if (label) link.label = label;

		return link;
	});

	if (links.length) basics.links = links;

	return basics;
}

/**
 * What promoters need to book a performer (`computer.localnetwork.section.defs#performer`), from PerformerFields, with
 * friendly errors. Every field is optional; a project also needs genres, which parseProfile checks.
 */
export function parsePerformer(form: FormData): PerformerSection {
	const genres = normalizeGenres(all(form, 'genres', 40).flatMap((value) => value.split(',')));
	const formats = all(form, 'formats').filter((value) => isOneOf(FORMATS, value));
	const setTypes = all(form, 'setTypes').filter((value) => isOneOf(SET_TYPES, value));
	const mixTitles = strings(form, 'mixTitle');
	const mixDurations = strings(form, 'mixDuration');
	const mixYears = strings(form, 'mixYear');
	const mixSets = strings(form, 'mixSet');

	const mixes: ProjectMix[] = [...linkRows(form, 'mixUrl', 'A mix link')].map(([url, row]) => {
		const durationMinutes = wholeNumber((mixDurations[row] ?? '').trim(), 'mixDuration', 1, 1440);
		const year = wholeNumber((mixYears[row] ?? '').trim(), 'mixYear', 1950, 2100);
		const set = setRef((mixSets[row] ?? '').trim());
		const mix: ProjectMix = { title: limit('Mix title', (mixTitles[row] ?? '').trim() || 'Mix', 120)!, url };

		if (durationMinutes !== undefined) mix.durationMinutes = durationMinutes;

		if (year !== undefined) mix.year = year;

		if (set) mix.set = set;

		return mix;
	});

	const lines = text(form, 'highlights', 20_000)
		.split('\n')
		.map((line) => line.trim())
		.filter(Boolean);

	const highlights = [...new Set(lines)].map((line) => limit('Each highlight', line, 120)!);

	if (highlights.length > 12) throw new UserError('Add up to 12 highlights');

	const setMin = optionalInt(form, 'setMin', 15, 720);
	const setMax = optionalInt(form, 'setMax', 15, 720);

	if (setMin && setMax && setMin > setMax) throw new UserError('Shortest set is longer than the longest set');
	const capacityMin = optionalInt(form, 'capacityMin', 0, 1_000_000);
	const capacityMax = optionalInt(form, 'capacityMax', 0, 1_000_000);

	if (capacityMin !== undefined && capacityMax !== undefined && capacityMin > capacityMax) {
		throw new UserError('Smallest room is bigger than the biggest room');
	}

	const fees = feeRows(form).flatMap((row): ProjectFee[] => {
		const label = limit('A fee label', row.label || undefined, 64);
		const currency = currencyCode(row.currency);
		const minAmount = wholeNumber(row.minAmount, 'feeMin', 0, 10_000_000);
		const maxAmount = wholeNumber(row.maxAmount, 'feeMax', 0, 10_000_000);

		if (!row.tier) {
			if (label || currency || minAmount !== undefined || maxAmount !== undefined)
				throw new UserError('Pick a fee tier for each fee, or clear its row');

			return [];
		}

		if (!isOneOf(FEE_TIERS, row.tier)) throw new UserError('Pick a fee tier');

		if (minAmount !== undefined && maxAmount !== undefined && minAmount > maxAmount)
			throw new UserError('Minimum fee is above the maximum');
		const fee: ProjectFee = { tier: row.tier };

		if (label) fee.label = label;

		if (currency) fee.currency = currency;

		if (minAmount !== undefined) fee.minAmount = minAmount;

		if (maxAmount !== undefined) fee.maxAmount = maxAmount;

		return [fee];
	});

	const radiusKm = optionalInt(form, 'travelRadiusKm', 0, 20_000);
	const international = form.get('international') === 'on';
	const travelNotes = limit('Travel notes', optionalText(form, 'travelNotes', 3000), 300);
	const performer: PerformerSection = {};
	const tagline = limit('Tagline', optionalText(form, 'tagline', 1400), 140);

	if (tagline) performer.tagline = tagline;

	if (genres.length) performer.genres = genres;

	if (radiusKm !== undefined || international || travelNotes) {
		performer.travel = {};

		if (radiusKm !== undefined) performer.travel.radiusKm = radiusKm;

		if (international) performer.travel.international = international;

		if (travelNotes) performer.travel.notes = travelNotes;
	}

	if (formats.length) performer.formats = formats;

	if (setTypes.length) performer.setTypes = setTypes;

	if (setMin || setMax) {
		performer.setLength = {};

		if (setMin) performer.setLength.minMinutes = setMin;

		if (setMax) performer.setLength.maxMinutes = setMax;
	}

	const activeSince = optionalInt(form, 'activeSince', 1950, new Date().getFullYear());

	if (activeSince) performer.activeSince = activeSince;

	if (capacityMin !== undefined || capacityMax !== undefined) {
		performer.capacity = {};

		if (capacityMin !== undefined) performer.capacity.min = capacityMin;

		if (capacityMax !== undefined) performer.capacity.max = capacityMax;
	}

	if (fees.length) performer.fees = fees;

	if (mixes.length) performer.mixes = mixes;

	if (highlights.length) performer.highlights = highlights;
	const lookingFor = limit('What you are looking for', optionalText(form, 'lookingFor', 5000), 500);

	if (lookingFor) performer.lookingFor = lookingFor;

	return performer;
}

/** Where a promoter books (`computer.localnetwork.section.defs#promoter`), from PromoterFields' city rows. */
export function parsePromoter(form: FormData): PromoterSection {
	const [cities, regions, countries] = ['bookCity', 'bookRegion', 'bookCountry'].map((field) =>
		strings(form, field).map((value) => value.trim())
	);

	const rows = Array.from({ length: Math.max(cities.length, regions.length, countries.length) }).flatMap(
		(_, row): Location[] => {
			const city = limit('City', cities[row] ?? '', 100);
			const country = (countries[row] ?? '').toUpperCase();
			const region = limit('Region', regions[row] || undefined, 100);

			if (!city && !country && !region) return [];

			if (!city) throw new UserError('Name each city you book in, or clear its row');

			if (!/^[A-Z]{2}$/.test(country))
				throw new UserError(`Use a two-letter country code for ${city} (like US, GB, or DE)`);

			return [region ? { city, region, country } : { city, country }];
		}
	);

	const seen = new Set<string>();

	const unique = rows.filter((place) => {
		const key = `${place.city.toLowerCase()}|${place.country}`;

		return !seen.has(key) && seen.add(key);
	});

	if (unique.length > 12) throw new UserError('Add up to 12 cities');

	return unique.length ? { cities: unique } : {};
}

/** Parses the shared project form into a lexicon-shaped profile, with friendly errors. */
export function parseProfile(form: FormData): ProjectProfile {
	const { name, ...basics } = parseBasics(form, 'Give the project a name');
	const kind = text(form, 'kind', 32) || 'solo';

	if (!isOneOf(PROJECT_KINDS, kind)) throw new UserError('Pick what kind of project this is');
	const { genres = [], ...performer } = parsePerformer(form);

	if (genres.length === 0) throw new UserError('Add at least one genre');
	const location = parseLocation(form, true, 'Add the city the project is based in')!;

	return { name, kind, genres, location, ...basics, ...performer };
}

export function parseDetails(form: FormData): ProjectDetails {
	const details: ProjectDetails = {};
	const email = optionalText(form, 'bookingEmail', 320);

	if (email) {
		if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) throw new UserError('That booking email does not look right');
		details.bookingEmail = email;
	}

	const phone = optionalText(form, 'bookingPhone', 64);

	if (phone) details.bookingPhone = phone;
	const rateNotes = limit('Rate notes', optionalText(form, 'rateNotes', 10_000), 1000);

	if (rateNotes) details.rateNotes = rateNotes;
	const techRider = limit('Tech rider', optionalText(form, 'techRider', 20_000), 2000);

	if (techRider) details.techRider = techRider;
	const hospitality = limit('Hospitality', optionalText(form, 'hospitality', 10_000), 1000);

	if (hospitality) details.hospitality = hospitality;

	return details;
}

export async function parseAvatar(form: FormData): Promise<{ bytes: Uint8Array; mimeType: string } | undefined> {
	const file = form.get('avatar');

	if (!(file instanceof File) || file.size === 0) return undefined;

	if (!AVATAR_TYPES.has(file.type)) throw new UserError('Upload a PNG, JPEG, or WebP image');

	if (file.size > AVATAR_MAX_BYTES) throw new UserError('Images can be at most 1 MB');
	const bytes = new Uint8Array(await file.arrayBuffer());

	if (!sniffImage(bytes, file.type)) throw new UserError("That file isn't the image type it claims to be");

	return { bytes, mimeType: file.type };
}

export type Upload = { bytes: Uint8Array; mimeType: string };

/**
 * Mix tracklists from the project form, aligned with `profile.mixes`. A newly chosen file is
 * validated and re-serialized (so only fields this site knows are published) and returned as an
 * upload; otherwise the mix keeps the tracklist it had (`mixTracklistCid`), but only when one of
 * `previous` already holds that blob, so a form can't point a mix at someone else's file.
 */
export async function withTracklists<T extends { mixes?: ProjectMix[] }>(
	form: FormData,
	profile: T,
	previous: ({ mixes?: ProjectMix[] } | null | undefined)[] = []
): Promise<{ profile: T; tracklists: (Upload | undefined)[] }> {
	const rows = linkRows(form, 'mixUrl', 'A mix link');
	const cids = strings(form, 'mixTracklistCid');
	const files = form.getAll('mixTracklist');
	// A file on a row parseProfile dropped would otherwise vanish without a word.
	const titles = strings(form, 'mixTitle');

	for (const [row, raw] of strings(form, 'mixUrl').entries()) {
		const file = files[row];

		if (!(file instanceof File) || file.size === 0) continue;

		if (!raw.trim()) throw new UserError('Add a link for the mix before attaching its tracklist');

		if (rows.get(toUrl(raw.trim(), 'A mix link')) !== row)
			throw new UserError(`“${titles[row]?.trim() || 'Mix'}” is listed twice. Add its tracklist to the first one`);
	}

	const known = new Map<string, BlobRefJson>();

	for (const p of previous)
		for (const m of p?.mixes ?? []) if (m.tracklist) known.set(m.tracklist.ref.$link, m.tracklist);
	const mixes = [...(profile.mixes ?? [])];
	const tracklists: (Upload | undefined)[] = [];

	for (const [index, mix] of mixes.entries()) {
		// The form row parseProfile took this mix from; its file and tracklist inputs share that row.
		const row = rows.get(mix.url);

		if (row === undefined) continue;
		const file = files[row];

		if (file instanceof File && file.size > 0) {
			if (file.size > MIX_REPORT_MAX_BYTES) throw new UserError(`The tracklist for “${mix.title}” is over 1 MB`);
			const result = parseMixReportText(new Uint8Array(await file.arrayBuffer()));

			if (!result.ok) throw new UserError(`The tracklist for “${mix.title}”: ${result.error}`);
			tracklists[index] = {
				bytes: new TextEncoder().encode(JSON.stringify(result.report)),
				mimeType: 'application/json'
			};
		} else {
			const kept = known.get(cids[row]?.trim() ?? '');

			if (kept) mixes[index] = { ...mix, tracklist: kept };
		}
	}

	return { profile: mixes.length ? { ...profile, mixes } : profile, tracklists };
}

/** Checks magic bytes so a renamed file can't masquerade as an image. */
export function sniffImage(bytes: Uint8Array, mimeType: string): boolean {
	const starts = (...sig: number[]) => sig.every((byte, index) => bytes[index] === byte);

	if (mimeType === 'image/png') return starts(0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a);

	if (mimeType === 'image/jpeg') return starts(0xff, 0xd8, 0xff);

	if (mimeType === 'image/webp')
		return (
			starts(0x52, 0x49, 0x46, 0x46) &&
			bytes[8] === 0x57 &&
			bytes[9] === 0x45 &&
			bytes[10] === 0x42 &&
			bytes[11] === 0x50
		);

	return false;
}

/** A place as the place picker's fields. */
function locationValues(location: Location | undefined) {
	return {
		city: location?.city,
		region: location?.region,
		country: location?.country,
		latE6: location?.latE6,
		lngE6: location?.lngE6
	} satisfies ProjectFormValues;
}

/** The basics as BasicsFields' values. */
export function basicsToValues(basics: Omit<BasicsSection, 'avatar'>) {
	return {
		name: basics.name,
		bio: basics.bio,
		pronouns: basics.pronouns,
		links: basics.links ?? [],
		...locationValues(basics.location)
	} satisfies ProjectFormValues;
}

/** A performer section (or a project's profile, which has its fields) as PerformerFields' values. */
export function performerToValues(performer: PerformerSection & Pick<ProjectProfile, 'fee'>) {
	return {
		tagline: performer.tagline,
		genres: (performer.genres ?? []).join(', '),
		travelRadiusKm: performer.travel?.radiusKm,
		international: performer.travel?.international,
		travelNotes: performer.travel?.notes,
		formats: performer.formats ?? [],
		setTypes: performer.setTypes ?? [],
		setMin: performer.setLength?.minMinutes,
		setMax: performer.setLength?.maxMinutes,
		activeSince: performer.activeSince,
		capacityMin: performer.capacity?.min,
		capacityMax: performer.capacity?.max,
		fees: feeLevels(performer).map((fee) => ({
			label: fee.label ?? '',
			tier: fee.tier,
			currency: fee.currency ?? '',
			minAmount: String(fee.minAmount ?? ''),
			maxAmount: String(fee.maxAmount ?? '')
		})),
		mixes: performer.mixes ?? [],
		highlights: (performer.highlights ?? []).join('\n'),
		lookingFor: performer.lookingFor
	} satisfies ProjectFormValues;
}

/** A promoter section as PromoterFields' city rows. */
export function promoterToValues(promoter: PromoterSection) {
	return {
		cities: (promoter.cities ?? []).map((place) => ({
			city: place.city,
			region: place.region ?? '',
			country: place.country
		}))
	} satisfies ProjectFormValues;
}

/** Turns a profile back into flat form values, for re-rendering forms. */
export function profileToValues(profile: ProjectProfile | undefined, details?: ProjectDetails | null) {
	if (!profile) return { ...details } satisfies ProjectFormValues;

	return {
		...basicsToValues(profile),
		kind: profile.kind,
		...performerToValues(profile),
		...details
	} satisfies ProjectFormValues;
}

/** Raw submitted values (minus files), echoed back into the form on errors. */
export function formValues(form: FormData) {
	const values: ProjectFormValues = {};

	for (const [key, value] of form.entries()) {
		if (value instanceof File) continue;

		if (key === 'formats' || key === 'setTypes') (values[key] ??= []).push(value);
		else if (
			[
				'linkKind',
				'linkUrl',
				'linkLabel',
				'mixTitle',
				'mixUrl',
				'mixDuration',
				'mixYear',
				'mixTracklistCid',
				'mixSet',
				'feeLabel',
				'feeTier',
				'feeCurrency',
				'feeMin',
				'feeMax',
				'bookCity',
				'bookRegion',
				'bookCountry'
			].includes(key)
		)
			continue;
		else values[key] = value;
	}

	const linkKinds = strings(form, 'linkKind');
	const linkLabels = strings(form, 'linkLabel');
	values.links = strings(form, 'linkUrl')
		.map((url, index) => {
			const link: ProjectLink = { kind: linkKinds[index] || 'other', url };

			if (linkLabels[index]) link.label = linkLabels[index];

			return link;
		})
		.filter((link) => link.url);
	const mixTitles = strings(form, 'mixTitle');
	const mixDurations = strings(form, 'mixDuration');
	const mixYears = strings(form, 'mixYear');
	const mixTracklists = strings(form, 'mixTracklistCid');
	const mixSets = strings(form, 'mixSet');
	values.mixes = strings(form, 'mixUrl')
		.map((url, index) => {
			const mix: ProjectMix = { title: mixTitles[index] ?? '', url };
			const [setUri, setCid] = (mixSets[index] ?? '').split(' ');

			if (mixDurations[index]) mix.durationMinutes = Number(mixDurations[index]);

			if (mixYears[index]) mix.year = Number(mixYears[index]);

			// Keep showing a tracklist the mix already had; a newly chosen file has to be chosen again.
			if (mixTracklists[index])
				mix.tracklist = { $type: 'blob', ref: { $link: mixTracklists[index] }, mimeType: 'application/json', size: 0 };

			if (setUri && setCid) mix.set = { uri: setUri, cid: setCid };

			return mix;
		})
		.filter((mix) => mix.url);
	values.fees = feeRows(form);
	const [bookRegions, bookCountries] = [strings(form, 'bookRegion'), strings(form, 'bookCountry')];
	values.cities = strings(form, 'bookCity').map((city, row) => ({
		city,
		region: bookRegions[row] ?? '',
		country: bookCountries[row] ?? ''
	}));
	values.international = form.get('international') === 'on';

	return values;
}
