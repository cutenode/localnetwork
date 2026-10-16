import { describe, expect, it } from 'vitest';
import { ACCESS_PRESETS, AUDIENCES, orientation, photoSize, presetFor, pressChecks, readRule, ruleFor } from './epk.ts';

const OPENNESS = { none: 0, review: 1, auto: 2 } as const;

describe('access presets', () => {
	it('run from open to closed, each no more open than the one before for any group', () => {
		expect(ACCESS_PRESETS[0].access).toEqual({
			bookedWith: 'auto',
			following: 'auto',
			promoters: 'auto',
			anyone: 'auto'
		});
		expect(ACCESS_PRESETS.at(-1)!.access).toEqual({
			bookedWith: 'none',
			following: 'none',
			promoters: 'none',
			anyone: 'none'
		});

		for (const [index, preset] of ACCESS_PRESETS.entries()) {
			if (index === 0) continue;

			for (const audience of AUDIENCES)
				expect(OPENNESS[preset.access[audience]]).toBeLessThanOrEqual(
					OPENNESS[ACCESS_PRESETS[index - 1].access[audience]]
				);
		}
	});

	it('are all different, and recognised from their rules', () => {
		for (const preset of ACCESS_PRESETS) expect(presetFor({ ...preset.access })?.id).toBe(preset.id);
		expect(presetFor({ bookedWith: 'none', following: 'auto', promoters: 'none', anyone: 'none' })).toBeNull();
	});
});

describe('ruleFor', () => {
	const access = { bookedWith: 'auto', following: 'review', promoters: 'none', anyone: 'none' } as const;

	it('gives someone the most open rule of the groups they are in', () => {
		expect(ruleFor(access, [])).toBe('none');
		expect(ruleFor(access, ['promoters', 'following'])).toBe('review');
		expect(ruleFor(access, ['following', 'bookedWith'])).toBe('auto');
	});

	it('reads a rule this app does not know as none', () => {
		expect(readRule('maybe')).toBe('none');
		expect(readRule('review')).toBe('review');
	});
});

describe('press photo checks', () => {
	it('sizes and shapes photos the way press asks for them', () => {
		expect(photoSize(3200, 1800)).toBe('press');
		expect(photoSize(1600, 1600)).toBe('web');
		expect(photoSize(800, 600)).toBe('small');
		expect(photoSize(undefined, 10)).toBeNull();
		expect([orientation(3200, 1800), orientation(2000, 3000), orientation(1000, 1020)]).toEqual([
			'landscape',
			'portrait',
			'square'
		]);
	});

	it('passes a set of three credited photos in every orientation at press size', () => {
		const set = [
			{ width: 3200, height: 1800, credit: 'Mara' },
			{ width: 2000, height: 3000, credit: 'Mara' },
			{ width: 3000, height: 3000, credit: 'Mara' }
		];

		expect(pressChecks(set).every((check) => check.done)).toBe(true);
		expect(pressChecks(set.slice(0, 2)).map((check) => check.done)).toEqual([false, false, true, true]);
		expect(pressChecks([]).some((check) => check.done)).toBe(false);
	});
});
