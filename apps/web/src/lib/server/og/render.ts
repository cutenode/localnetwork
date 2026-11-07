import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { initWasm, Resvg } from '@resvg/resvg-wasm';
import satori, { type Font } from 'satori';
import { html } from 'satori-html';
import sharp from 'sharp';
import type { Component } from 'svelte';
import { render } from 'svelte/server';
import { CARD } from '#lib/og/style.ts';
import { UNDRAWABLE } from './drawable';

/** The most a card may weigh: WhatsApp shows no picture over 600 KB. */
export const MAX_CARD_BYTES = 600_000;

/** A card's bytes, on an ArrayBuffer of their own so a Response takes them. */
export type Png = Uint8Array<ArrayBuffer>;

/** Static cuts of the site's two typefaces: Satori draws a variable font at its default weight only. */
const FONTS = [
	['Instrument Sans', 400, '@fontsource/instrument-sans/files/instrument-sans-latin-400-normal.woff'],
	['Instrument Sans', 400, '@fontsource/instrument-sans/files/instrument-sans-latin-ext-400-normal.woff'],
	['Instrument Sans', 600, '@fontsource/instrument-sans/files/instrument-sans-latin-600-normal.woff'],
	['Instrument Sans', 600, '@fontsource/instrument-sans/files/instrument-sans-latin-ext-600-normal.woff'],
	['JetBrains Mono', 400, '@fontsource/jetbrains-mono/files/jetbrains-mono-latin-400-normal.woff']
] as const;

/**
 * Palettes a card over MAX_CARD_BYTES is re-encoded with. A PNG stores 17 to 256 colours at 8 bits a pixel, so fewer
 * than 256 saves little until 16, at 4 bits: random noise came to 759 KB at 256 or 64 colours and 379 KB at 16.
 */
const PALETTES = [256, 16];

/**
 * How hard sharp works on a palette, 1 to 10. Past 4 a 1200×630 card took four times as long (1.4 s for random noise at
 * 7) for no smaller file.
 */
const PALETTE_EFFORT = 4;

const resolve = createRequire(import.meta.url).resolve;

let fonts: Promise<Font[]> | undefined;

let resvg: Promise<void> | undefined;

/**
 * The fonts and resvg's wasm, read from the installed packages on the first render. Drawing a card asks nothing of the
 * network: outbound requests go through the app's guarded fetch, and a card has nothing to fetch.
 */
const prepare = () =>
	Promise.all([
		(fonts ??= Promise.all(
			FONTS.map(async ([name, weight, file]) => ({
				name,
				weight,
				style: 'normal' as const,
				data: await readFile(resolve(file))
			}))
		)),
		(resvg ??= readFile(resolve('@resvg/resvg-wasm/index_bg.wasm')).then(initWasm))
	]);

/**
 * The markup as Satori's element tree, without the characters the fonts can't draw (see UNDRAWABLE). Everything else
 * in a card's markup (tags, styles, base64) is ASCII, so they're taken out of the markup as a whole. It goes to
 * satori-html as a template's one string: given a plain string,
 * ultrahtml's `html` tag loops over it a character at a time, quadratic in its length, which took 1.7 s for a card
 * with a press photo in it. An empty element needs `display: flex`: satori-html gives it `children: []`, which Satori
 * reads as several children and refuses anywhere else.
 */
function toVNode(markup: string) {
	const drawable = markup.replaceAll('\n', '').replace(UNDRAWABLE, '');

	// Satori takes React's element type, which has a key.
	return { ...html(Object.assign([drawable], { raw: [drawable] })), key: null };
}

/**
 * `component` drawn as a PNG: Svelte renders its markup, Satori lays it out as an SVG, and resvg draws that. Characters
 * the fonts lack (emoji, scripts other than Latin) are left out rather than fetched from anywhere.
 */
export async function renderCard<Props extends Record<string, unknown>>(
	component: Component<Props>,
	props: Props
): Promise<Png> {
	const [loaded] = await prepare();
	const { body } = render(component, { props });

	// No `loadAdditionalAsset`: without one, Satori looks nowhere for glyphs its fonts lack.
	const svg = await satori(toVNode(body), { ...CARD, fonts: loaded });

	const drawn = new Resvg(svg, { fitTo: { mode: 'width', value: CARD.width } });
	const image = drawn.render();
	const png = new Uint8Array(image.asPng());
	image.free();
	drawn.free();

	return fitPng(png);
}

/**
 * `png` within `max` bytes: re-encoded with fewer and fewer colours until it fits (a photo behind a card can take it
 * over), or an error when even the smallest palette doesn't, so no app is sent a picture it won't show.
 */
export async function fitPng(png: Png, max = MAX_CARD_BYTES): Promise<Png> {
	if (png.byteLength <= max) return png;

	for (const colours of PALETTES) {
		const smaller = new Uint8Array(await sharp(png).png({ palette: true, colours, effort: PALETTE_EFFORT }).toBuffer());

		if (smaller.byteLength <= max) return smaller;
	}

	throw new Error(`a card is over ${max} bytes even with ${PALETTES.at(-1)} colours`);
}
