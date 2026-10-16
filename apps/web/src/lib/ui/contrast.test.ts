import { describe, expect, test } from 'vitest';
import colors from '../../../../../packages/tokens/colors.json' with { type: 'json' };
import { contrast, onColour } from './contrast.ts';

describe('contrast', () => {
	test('matches the WCAG ratios for black and white', () => {
		expect(contrast('#000000', '#ffffff')).toBeCloseTo(21, 5);
		expect(contrast('#ffffff', '#ffffff')).toBe(1);
	});
});

describe('onColour', () => {
	test('every track colour gets an ink that reads at 4.5:1 or better', () => {
		for (const hex of Object.values(colors.trackColors)) {
			const { background, ink } = onColour(hex);
			expect(contrast(background, ink), hex).toBeGreaterThanOrEqual(4.5);
		}
	});

	test('keeps a colour that already reads, picking the better ink', () => {
		expect(onColour(colors.trackColors.yellow)).toEqual({ background: colors.trackColors.yellow, ink: '#09090b' });
		expect(onColour(colors.trackColors.blue)).toEqual({ background: colors.trackColors.blue, ink: '#f4f4f5' });
	});

	test('darkens purple, which reads with neither ink', () => {
		const { background, ink } = onColour(colors.trackColors.purple);
		expect(background).not.toBe(colors.trackColors.purple);
		expect(ink).toBe('#f4f4f5');
	});
});
