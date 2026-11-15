import { expect, it } from 'vitest';
import { NEON_TIDE } from '#lib/testing/link-matrix.ts';
import { sampleNight } from './landing.ts';

it('asks the matcher about nothing for a listing without a genre', () => {
	expect(sampleNight({ ...NEON_TIDE, profile: { ...NEON_TIDE.profile, genres: [] } })).toBeNull();
});
