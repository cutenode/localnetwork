import manifest from './captures/stories.json';

/**
 * locallibrary's terminal widgets as its tests draw them (apps/locallibrary/src/tui/ui/tests/stories.rs):
 * for each story, the bytes a terminal is sent in each palette, a still as `.ansi` and one that
 * moves as an asciicast v2 `.cast`, and its size in cells.
 */
export type Palette = keyof typeof manifest.pages;

export type StoryId = keyof typeof manifest.stories;

export type Capture = (typeof manifest.stories)[StoryId] & {
	/** The colours the terminal itself has, which the widget leaves to it. */
	page: { fg: string; bg: string };
	/** The `.ansi` bytes, or the `.cast` recording for one that moves. */
	bytes: string;
};

// SAFETY: the keys of the imported JSON are the keys its type has.
export const PALETTES = Object.keys(manifest.pages) as Palette[];

const files = import.meta.glob<string>('./captures/**/*.{ansi,cast}', { query: '?raw', import: 'default' });

/** Story `id` in `palette`, read when it's drawn. */
export async function capture(id: StoryId, palette: Palette): Promise<Capture> {
	const story = manifest.stories[id];
	const bytes = await files[`./captures/${id}.${palette}.${story.moves ? 'cast' : 'ansi'}`]();

	return { ...story, page: manifest.pages[palette], bytes };
}
