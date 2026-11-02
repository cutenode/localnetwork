import { readFileSync, writeFileSync } from 'node:fs';
import { expect, test } from 'vitest';
import colors from '../../../../../packages/tokens/colors.json' with { type: 'json' };

/**
 * packages/tokens/data-colors.css is colors.json as Tailwind theme colours, so the site, the
 * extension, the player and the HTML report draw DJ data the same way. Regenerate it after editing
 * colors.json.
 */
const RERUN = 'TOKENS_BLESS=1 pnpm -C apps/web vitest run --project server src/lib/ui/data-colors.test.ts';

const path = new URL('../../../../../packages/tokens/data-colors.css', import.meta.url);

const group = (prefix: string, values: Record<string, string>) =>
	Object.entries(values).map(([name, hex]) => `\t--color-${prefix}${name}: ${hex};`);

const expected = [
	`/* Generated from packages/tokens/colors.json: don't edit by hand. Regenerate with \`${RERUN}\`. */`,
	'@theme {',
	...group('band-', { low: colors.bands.low, mid: colors.bands.mid, high: colors.bands.high }),
	...group('phrase-', colors.phrases),
	...group('cue-', colors.cues),
	...group('track-', colors.trackColors),
	...group('', colors.status.dark),
	`\t--color-playhead: ${colors.playhead};`,
	'}',
	'',
	'/* On the light palette (theme.css) the phrases, cues, good, warning and critical take their light values. */',
	':root {',
	'\t@variant light {',
	...group('phrase-', colors.phrasesLight).map((line) => `\t${line}`),
	...group('cue-', colors.cuesLight).map((line) => `\t${line}`),
	...group('', colors.status.light).map((line) => `\t${line}`),
	'\t}',
	'}',
	''
].join('\n');

test('packages/tokens/data-colors.css matches packages/tokens/colors.json', () => {
	if (process.env.TOKENS_BLESS) writeFileSync(path, expected);
	expect(
		readFileSync(path, 'utf8'),
		`packages/tokens/data-colors.css is out of date. Regenerate it with \`${RERUN}\``
	).toBe(expected);
});
