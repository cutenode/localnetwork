// Matching typed words against a track's text, for the search box the
// Cover Art and Manager panels share (see FilterBar.svelte).

// Lower-case, without accents, single-spaced — so "Bicép" is found by "bicep"
// and "  Glue " by "glue".
export const normalize = (text: string | null | undefined) =>
	String(text ?? '')
		.toLowerCase()
		.normalize('NFKD')
		.replace(/\p{M}+/gu, '')
		.replace(/\s+/g, ' ')
		.trim();

// True when every word of `query` occurs somewhere in `text`, in any order:
// "glue bicep" finds "Bicep — Glue". An empty query matches everything.
export const matches = (text: string, query: string) => {
	const haystack = normalize(text);

	return normalize(query)
		.split(' ')
		.filter(Boolean)
		.every((word) => haystack.includes(word));
};

// The "Hide downloaded" toggle is part of the exported state file, under this
// key (state-transfer.js, and the schema in docs/).
export const HIDE_DOWNLOADED_PREF = 'pref:hide-downloaded';
