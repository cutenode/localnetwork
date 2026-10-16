import { readFileSync, writeFileSync } from 'node:fs';
import { expect, test } from 'vitest';
import colors from '../../../../../packages/tokens/colors.json' with { type: 'json' };

/**
 * data-colors.css is packages/tokens/colors.json as Tailwind theme colours, so the site, the
 * player and the HTML report draw DJ data the same way. Regenerate it after editing colors.json.
 */
const RERUN = 'TOKENS_BLESS=1 pnpm vitest run --project server src/lib/ui/data-colors.test.ts';

const path = new URL('./data-colors.css', import.meta.url);

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
	''
].join('\n');

test('data-colors.css matches packages/tokens/colors.json', () => {
	if (process.env.TOKENS_BLESS) writeFileSync(path, expected);
	expect(readFileSync(path, 'utf8'), `src/lib/ui/data-colors.css is out of date. Regenerate it with \`${RERUN}\``).toBe(
		expected
	);
});
