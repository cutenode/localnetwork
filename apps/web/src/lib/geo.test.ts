import { describe, expect, it } from 'vitest';
import { formatDistance, isValidLatLng, toE6 } from './geo';

describe('geo', () => {
	it('converts to integer microdegrees', () => {
		expect(toE6(52.520008)).toBe(52520008);
	});

	it('validates coordinates', () => {
		expect(isValidLatLng(0, 0)).toBe(true);
		expect(isValidLatLng(91, 0)).toBe(false);
		expect(isValidLatLng(0, Number.NaN)).toBe(false);
	});

	it('formats distances', () => {
		expect(formatDistance(0.4)).toBe('under 1 km');
		expect(formatDistance(4.26)).toBe('4.3 km');
		expect(formatDistance(1234.4)).toBe('1,234 km');
	});
});
