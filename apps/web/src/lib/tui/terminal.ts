/**
 * The font and cell size every captured terminal is drawn at, the same whatever the machine: the
 * site's mono face, loaded before a terminal measures its cells, at a fixed size and line height.
 */
export const FONT = { family: '"JetBrains Mono Variable", monospace', size: 14, lineHeight: 1 } as const;

/** Resolves once the font is ready to measure. */
export const fontLoaded = () => document.fonts.load(`${FONT.size}px ${FONT.family}`);
