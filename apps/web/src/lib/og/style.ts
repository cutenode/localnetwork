/**
 * What the link-preview cards are drawn with. Satori lays them out from inline styles only (no Tailwind, no custom
 * properties), so the theme's values are repeated here from packages/tokens/theme.css.
 */

/** Every card's size: the 1.91:1 picture Open Graph and X's large card show. */
export const CARD = { width: 1200, height: 630 } as const;

export const ink = {
	canvas: '#09090b',
	raised: '#111114',
	well: '#16161a',
	line: '#1f1f24',
	lineStrong: '#2c2c33',
	fg: '#f4f4f5',
	fgSoft: '#d4d4d8',
	fgMuted: '#a1a1aa',
	fgSubtle: '#8b8b94'
} as const;

/**
 * The static fonts the server renders with come first (Satori reads a variable font at its default weight only), then
 * the site's variable ones, which Storybook has.
 */
export const SANS = "'Instrument Sans', 'Instrument Sans Variable', sans-serif";

export const MONO = "'JetBrains Mono', 'JetBrains Mono Variable', monospace";

/** Up to `lines` lines of text, cut with an ellipsis, in a way both Satori and browsers read. */
export const clamp = (lines: number) =>
	`display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: ${lines}; overflow: hidden; text-overflow: ellipsis; word-break: break-word;`;

/** A name's size: as large as fits two lines of the space it's given. */
export function nameSize(name: string, sizes: readonly [chars: number, px: number][]): number {
	return sizes.find(([chars]) => name.length <= chars)?.[1] ?? sizes.at(-1)![1];
}
