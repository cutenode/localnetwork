/**
 * A small genre taxonomy used for autocomplete and, through the HappyView
 * bundle, for scoring how well a project's sound fits a show. Projects may
 * also use free-form tags; those only match exactly.
 */
export type GenreFamily = {
	id: string;
	label: string;
	genres: string[];
	/** Families that share audiences, scored as loosely related. */
	near?: string[];
};

export const GENRE_FAMILIES: GenreFamily[] = [
	{
		id: 'house',
		label: 'House',
		genres: [
			'house',
			'deep house',
			'tech house',
			'progressive house',
			'afro house',
			'chicago house',
			'soulful house',
			'lo-fi house',
			'ghetto house',
			'ballroom',
			'french house',
			'nu-disco'
		],
		near: ['disco', 'techno', 'garage', 'amapiano']
	},
	{
		id: 'techno',
		label: 'Techno',
		genres: [
			'techno',
			'minimal',
			'hard techno',
			'industrial techno',
			'acid',
			'dub techno',
			'melodic techno',
			'detroit techno',
			'hypnotic techno',
			'schranz'
		],
		near: ['house', 'electro', 'trance', 'hardcore']
	},
	{
		id: 'garage',
		label: 'Garage & UK',
		genres: ['uk garage', '2-step', 'speed garage', 'bassline', 'uk funky', 'grime'],
		near: ['house', 'bass', 'drum and bass']
	},
	{
		id: 'bass',
		label: 'Bass & Breaks',
		genres: [
			'dubstep',
			'uk bass',
			'breaks',
			'breakbeat',
			'footwork',
			'juke',
			'jersey club',
			'baltimore club',
			'halftime',
			'riddim'
		],
		near: ['drum and bass', 'garage', 'electro', 'hip-hop']
	},
	{
		id: 'drum and bass',
		label: 'Drum & Bass',
		genres: ['drum and bass', 'jungle', 'liquid', 'neurofunk', 'jump up', 'minimal drum and bass'],
		near: ['bass', 'garage']
	},
	{
		id: 'electro',
		label: 'Electro',
		genres: ['electro', 'electroclash', 'miami bass', 'italo', 'synth-pop'],
		near: ['techno', 'bass', 'disco']
	},
	{
		id: 'trance',
		label: 'Trance',
		genres: ['trance', 'psytrance', 'progressive trance', 'hard trance', 'goa', 'uplifting trance'],
		near: ['techno', 'edm', 'hardcore']
	},
	{
		id: 'hardcore',
		label: 'Hard Dance',
		genres: ['hardcore', 'gabber', 'happy hardcore', 'hardstyle', 'frenchcore', 'speedcore'],
		near: ['techno', 'trance']
	},
	{
		id: 'disco',
		label: 'Disco, Funk & Soul',
		genres: ['disco', 'funk', 'soul', 'boogie', 'edits', 'italo disco', 'cosmic'],
		near: ['house', 'electro', 'open format']
	},
	{
		id: 'amapiano',
		label: 'African & Diaspora',
		genres: ['amapiano', 'afrobeats', 'gqom', 'kuduro', 'baile funk', 'afro tech', 'highlife'],
		near: ['house', 'dancehall', 'latin']
	},
	{
		id: 'dancehall',
		label: 'Caribbean',
		genres: ['dancehall', 'reggae', 'soca', 'dub', 'reggaeton'],
		near: ['amapiano', 'latin', 'hip-hop']
	},
	{
		id: 'latin',
		label: 'Latin',
		genres: ['latin', 'cumbia', 'dembow', 'salsa', 'bachata', 'merengue', 'latin house'],
		near: ['dancehall', 'amapiano', 'open format']
	},
	{
		id: 'hip-hop',
		label: 'Hip-Hop & R&B',
		genres: ['hip-hop', 'rap', 'trap', 'r&b', 'drill', 'boom bap', 'phonk'],
		near: ['bass', 'dancehall', 'open format']
	},
	{
		id: 'open format',
		label: 'Open Format',
		genres: ['open format', 'top 40', 'pop', 'throwbacks', 'mashups', 'wedding', 'corporate'],
		near: ['hip-hop', 'disco', 'edm', 'latin']
	},
	{
		id: 'edm',
		label: 'EDM & Club',
		genres: ['edm', 'big room', 'future bass', 'electro house', 'moombahton', 'hyperpop'],
		near: ['trance', 'open format', 'bass']
	},
	{
		id: 'downtempo',
		label: 'Downtempo & Experimental',
		genres: ['ambient', 'downtempo', 'experimental', 'leftfield', 'balearic', 'idm', 'jazz', 'world'],
		near: ['disco', 'electro']
	},
	{
		id: 'rock',
		label: 'Rock & Alternative',
		genres: ['indie', 'rock', 'punk', 'post-punk', 'new wave', 'emo', 'metal', 'goth'],
		near: ['open format', 'electro']
	}
];

export const KNOWN_GENRES: string[] = GENRE_FAMILIES.flatMap((family) => family.genres).sort();

/** A Map, so a genre like `constructor` can't resolve to an `Object.prototype` member. */
export const GENRE_ALIASES: ReadonlyMap<string, string> = new Map([
	['dnb', 'drum and bass'],
	['d&b', 'drum and bass'],
	['drum & bass', 'drum and bass'],
	['drum n bass', 'drum and bass'],
	['ukg', 'uk garage'],
	['garage', 'uk garage'],
	['hip hop', 'hip-hop'],
	['hiphop', 'hip-hop'],
	['rnb', 'r&b'],
	['r and b', 'r&b'],
	['nudisco', 'nu-disco'],
	['nu disco', 'nu-disco'],
	['psy', 'psytrance'],
	['psy-trance', 'psytrance'],
	['2step', '2-step'],
	['2 step', '2-step'],
	['top40', 'top 40'],
	['lofi house', 'lo-fi house'],
	['lo fi house', 'lo-fi house'],
	['afro-house', 'afro house'],
	['tech-house', 'tech house'],
	['deep-house', 'deep house']
]);

/** Lowercase, trim, collapse whitespace, and apply common aliases. */
export function normalizeGenre(input: string): string {
	const cleaned = input
		.normalize('NFKC')
		.toLowerCase()
		.replace(/[\s_]+/g, ' ')
		.trim();

	return GENRE_ALIASES.get(cleaned) ?? cleaned;
}

const utf8 = new TextEncoder();

/** Normalized genres without blanks or duplicates, at most `max`. Lexicons cap a genre at 48 UTF-8 bytes. */
export function normalizeGenres(inputs: Iterable<string>, max = 12): string[] {
	const out: string[] = [];

	for (const input of inputs) {
		const genre = normalizeGenre(input);

		if (genre && utf8.encode(genre).length <= 48 && !out.includes(genre)) out.push(genre);

		if (out.length >= max) break;
	}

	return out;
}
