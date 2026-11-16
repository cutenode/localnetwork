// Every colour the extension writes words in, against what it writes them on,
// at WCAG AA's 4.5:1, in the light theme and the dark one.
//
// jsdom lays nothing out and paints nothing, so axe's colour-contrast rule
// can't run in the panels' audits (a11y.test.ts). What it would measure comes
// from a few places, and each is read here as written: the theme's tokens
// (packages/tokens), the content script's stylesheet, and the badges' colours.
// @vitest-environment node
import { describe, it, expect } from 'vitest';
import Color from 'colorjs.io';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { NETWORKS, linksOf } from '../src/tools/artists/profiles.ts';
import { SEARCH_SITES } from '../src/lib/search-sites.ts';

const read = (path: string) => readFileSync(resolve(import.meta.dirname, '..', path), 'utf8');

const ratio = (a: string, b: string) => new Color(a).contrast(new Color(b), 'WCAG21');

/** The `--color-*` values in the first block after `opener`. */
function tokens(css: string, opener: string) {
	const start = css.indexOf(opener);

	if (start === -1) throw new Error(`no ${opener}`);
	const block = css.slice(start, css.indexOf('}', start));

	return Object.fromEntries(
		[...block.matchAll(/--color-([a-z-]+):\s*(#[0-9a-f]{6});/g)].map(([, name, hex]) => [name, hex])
	);
}

const theme = read('../../packages/tokens/theme.css');

const data = read('../../packages/tokens/data-colors.css');

// Light values override dark ones, so the light palette is the dark one with
// the light block laid over it.
const dark = { ...tokens(theme, '@theme {'), ...tokens(data, '@theme {') };

const light = { ...dark, ...tokens(theme, '@variant light {'), ...tokens(data, '@variant light {') };

/** What text is written in: the text steps, links, and the status colours. */
const INKS = ['fg', 'fg-soft', 'fg-muted', 'fg-subtle', 'link', 'good', 'warning', 'critical'];

/** What it is written on: every surface, darkest to lightest. */
const SURFACES = ['canvas', 'sunken', 'surface', 'raised', 'well'];

describe.each([
	['dark', dark],
	['light', light]
])('the %s theme', (_name, palette) => {
	it('reads every text colour at 4.5:1 or better on every surface', () => {
		for (const ink of INKS) {
			for (const surface of SURFACES) {
				expect(ratio(palette[ink], palette[surface]), `${ink} on ${surface}`).toBeGreaterThanOrEqual(4.5);
			}
		}
	});

	it('reads the inverse text on the main text colour and on good and critical', () => {
		for (const ground of ['fg', 'good', 'critical']) {
			expect(ratio(palette['fg-inverse'], palette[ground]), ground).toBeGreaterThanOrEqual(4.5);
		}
	});
});

describe('the content script on SoundCloud', () => {
	const css = read('content.css').replace(/\/\*[\s\S]*?\*\//g, '');

	/** A declaration of the rule whose selector list has `selector` in it. */
	function declared(selector: string, property: string) {
		for (const [, selectors, body] of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
			if (!selectors.split(',').some((one) => one.trim() === selector)) continue;
			const value = body.match(new RegExp(`(?:^|;|\\s)${property}:\\s*([^;]+);`))?.[1];

			if (value) return value.trim();
		}

		throw new Error(`no ${property} for ${selector}`);
	}

	// A see-through pill sits on cover art, which can be anything, so it is
	// laid over white and over black and read on both.
	const over = (colour: string, ground: string) => {
		const top = new Color(colour).to('srgb');
		const base = new Color(ground).to('srgb');

		const [r, g, b] = top.coords.map((c, i) => (c ?? 0) * top.alpha + (base.coords[i] ?? 0) * (1 - top.alpha));

		return `color(srgb ${r} ${g} ${b})`;
	};

	it.each([
		['the download button', '.tbx-dl.tbx-dl', '.tbx-dl.tbx-dl'],
		['the download button, hovered', '.tbx-dl:hover', '.tbx-dl:hover'],
		['the queue button', '.tbx-queue.tbx-queue', '.tbx-queue.tbx-queue'],
		['the queue button, hovered', '.tbx-queue.tbx-queue:hover', '.tbx-queue.tbx-queue'],
		[
			'the queue button, hovered to unqueue',
			".tbx-queue.tbx-queue[data-state$=':yes']:hover",
			".tbx-queue.tbx-queue[data-state$=':yes']:hover"
		],
		['the downloaded pill', '.tbx-mark__pill', '.tbx-mark__pill'],
		['the nodl pill', '.tbx-mark--nodl .tbx-mark__pill', '.tbx-mark--nodl .tbx-mark__pill']
	])('reads %s at 4.5:1 or better', (_name, backgroundOf, colourOf) => {
		const ink = declared(colourOf, 'color');

		for (const art of ['#ffffff', '#000000']) {
			const ground = over(declared(backgroundOf, 'background'), art);
			expect(ratio(ink, ground), `on ${art}`).toBeGreaterThanOrEqual(4.5);
		}
	});
});

describe('the badges', () => {
	it('reads every network’s letters at 4.5:1 or better', () => {
		const links = linksOf({
			at: 1,
			links: Object.keys(NETWORKS).map((id) => ({
				network: id,
				url: id === 'email' ? 'mailto:someone@example.org' : `https://links.example/${id}`
			}))
		});

		expect(links.map((link) => link.network).sort()).toEqual(Object.keys(NETWORKS).sort());

		for (const { network, style } of links) {
			const [, background, color] = style.match(/^background:(#[0-9a-f]+);color:(#[0-9a-f]+)$/)!;
			expect(ratio(color, background), network).toBeGreaterThanOrEqual(4.5);
		}
	});

	it('reads every search site’s letters at 4.5:1 or better', () => {
		expect(SEARCH_SITES.length).toBeGreaterThan(0);

		for (const site of SEARCH_SITES) expect(ratio(site.color, site.background), site.id).toBeGreaterThanOrEqual(4.5);
	});
});
