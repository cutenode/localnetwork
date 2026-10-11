// Title Case, for the three fields of an opened row that are words rather
// than addresses — the title, the artist and the genre.
//
// What SoundCloud hands over is whatever the uploader typed, which is a
// tracklist in four house styles at once: "glue", "Glue", "GLUE (ORIGINAL
// MIX)". Somebody pasting those into one document wants one of them, and this
// is the switch that says so (title-case.svelte.ts, settings sheet).
//
// A converter like this can only guess, so what it guesses is written down
// here rather than left to be discovered a track at a time. Two rules, and the
// first is the one that keeps it honest:
//
// 1. A capital the uploader typed is a claim about the word — SBTRKT, DnB,
//    McCoy, EDM — and a claim nobody here can check. So a word that carries
//    one anywhere is left exactly as it was typed, and this only ever *raises*
//    a lower-case word.
//
// 2. Unless the whole field is in capitals, which is the one case where rule 1
//    has nothing to preserve: if every letter is a capital then no capital
//    says anything, and the field is re-cased from scratch. The short words
//    whose capitals really are the word (KEPT, below) survive that too, since
//    "HARDCORE EDM MIX" is not asking for "Hardcore Edm Mix".
//
// Which leaves the field that was shouted at readable, the artist who spells
// their name in capitals spelt their way in every other field, and nothing
// lower-cased that somebody had raised on purpose.

/**
 * The words a title keeps in lower case in the middle of it.
 *
 * Articles, conjunctions and the short prepositions — what makes "Fly Me to
 * the Moon" a title rather than a row of capitals. Never the first or the last
 * word, which are raised wherever they land: a title does not open or close in
 * lower case.
 */
const SMALL = new Set([
	'a',
	'an',
	'and',
	'as',
	'at',
	'but',
	'by',
	'for',
	'from',
	'in',
	'into',
	'nor',
	'of',
	'on',
	'onto',
	'or',
	'over',
	'per',
	'the',
	'to',
	'via',
	'vs',
	'with'
]);

/**
 * The words whose capitals are the word rather than a raised voice.
 *
 * Only consulted for a field that is entirely in capitals, where rule 2 is
 * about to re-case everything: these are what a tracklist is full of and what
 * nobody wants written as "Dj", "Ep" or "Vip". Short, and deliberately not a
 * dictionary — a name this misses is a name that keeps its capitals in every
 * field that was not shouted, which is most of them.
 */
const KEPT = new Set([
	'aka',
	'bpm',
	'dj',
	'dnb',
	'edm',
	'ep',
	'id',
	'idm',
	'la',
	'lp',
	'mc',
	'nyc',
	'ost',
	'rmx',
	'tv',
	'uk',
	'ukg',
	'us',
	'usa',
	'vip',
	'ii',
	'iii',
	'iv',
	'vi',
	'vii',
	'viii',
	'ix'
]);

// A word, for casing: letters and digits, carrying any apostrophes inside it
// ("don't", "o’clock") but not the ones around it ("rock 'n' roll", where the
// word is the n). Everything between two words — spaces, hyphens, slashes,
// brackets, ampersands — is a separator and comes through untouched, which is
// what makes "hip-hop" into "Hip-Hop" and leaves "Drum & Bass" spaced as it
// was typed.
const WORD = /[\p{L}\p{N}][\p{L}\p{N}'’]*/gu;

const raise = (word: string) => word[0].toUpperCase() + word.slice(1);

/**
 * One field, in Title Case — see the two rules at the top of this file.
 *
 * A field with no word in it at all ('', '—', '?') comes back as it went in:
 * there is nothing to raise, and an empty field is the caller's own business
 * (rowFacts draws it as "none" or "unknown").
 *
 * @param text what the uploader typed
 */
export function titleCase(text: string) {
	const words = text.match(WORD);

	if (!words) return text;

	// No lower-case letter anywhere: the whole field is in capitals, so its
	// capitals carry nothing and rule 2 applies to every word in it.
	const shouted = !/\p{Ll}/u.test(text);
	const last = words.length - 1;
	let nth = -1;

	return text.replace(WORD, (word) => {
		nth += 1;
		const lower = word.toLowerCase();

		// Rule 2's exceptions, and rule 1 itself: both say "as it was typed".
		if (shouted ? KEPT.has(lower) : /\p{Lu}/u.test(word)) return word;

		// The small words, but never at either end.
		if (nth > 0 && nth < last && SMALL.has(lower)) return lower;

		return raise(lower);
	});
}
