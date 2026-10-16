/** The values of --color-fg and --color-fg-inverse in theme.css. */
const LIGHT = '#f4f4f5';

const DARK = '#09090b';

const channels = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));

/** WCAG relative luminance of a #rrggbb colour. */
function luminance(hex: string): number {
	const [r, g, b] = channels(hex).map((v) => {
		const c = v / 255;

		return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
	});

	return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(a: string, b: string): number {
	const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);

	return (hi + 0.05) / (lo + 0.05);
}

const scale = (hex: string, f: number) =>
	`#${channels(hex)
		.map((v) =>
			Math.round(v * f)
				.toString(16)
				.padStart(2, '0')
		)
		.join('')}`;

/** A DJ-data colour made readable, and the ink to write on it with. */
export type Swatch = { background: string; ink: string };

/**
 * Text on a DJ-data colour (a track colour behind a monogram, a lane or a key): whichever of the
 * light or dark ink reads better, with the colour darkened a step at a time when neither reaches
 * 4.5:1, as track purple needs.
 */
export function onColour(hex: string): Swatch {
	const pick = (background: string) => (contrast(background, LIGHT) >= contrast(background, DARK) ? LIGHT : DARK);

	let factor = 1;
	let background = hex;
	let ink = pick(background);

	// Ends by black at the latest, where the light ink reads at over 4.5:1.
	while (contrast(background, ink) < 4.5) {
		factor -= 0.04;
		background = scale(hex, factor);
		ink = pick(background);
	}

	return { background, ink };
}
