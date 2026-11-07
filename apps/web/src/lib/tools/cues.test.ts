import { describe, expect, it } from 'vitest';
import { cuePoints } from './cues.ts';

describe('cuePoints', () => {
	it('spreads the cues evenly over a track with no phrases', () => {
		expect(cuePoints(4, 400, [])).toEqual([50, 150, 250, 350]);
	});

	it('moves a cue onto a phrase that starts close by', () => {
		expect(cuePoints(4, 400, [{ start: 64 }, { start: 240 }])).toEqual([64, 150, 240, 350]);
	});

	it('leaves a cue where it is when the nearest phrase is too far off to keep the cues apart', () => {
		expect(cuePoints(2, 200, [{ start: 0 }, { start: 100 }])).toEqual([50, 150]);
	});
});
