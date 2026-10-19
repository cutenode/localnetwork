import { afterEach, describe, expect, it, vi } from 'vitest';
import { atVenue, venueTime } from './venue-time';

const BERLIN = { latE6: 52_520_000, lngE6: 13_405_000 };

const NEW_YORK = { latE6: 40_712_800, lngE6: -74_006_000 };

const KOLKATA = { latE6: 22_572_600, lngE6: 88_363_900 };

describe('venueTime', () => {
	afterEach(() => vi.unstubAllEnvs());

	it("writes the venue's clock with the venue's offset that day, whatever zone the server is in", () => {
		vi.stubEnv('TZ', 'America/Los_Angeles');
		expect(venueTime('2026-11-07', '23:00', BERLIN)).toBe('2026-11-07T23:00:00+01:00');
		expect(venueTime('2026-07-04', '23:00', BERLIN)).toBe('2026-07-04T23:00:00+02:00');
		expect(venueTime('2026-11-07', '21:30', NEW_YORK)).toBe('2026-11-07T21:30:00-05:00');
		expect(venueTime('2026-11-07', '21:30', KOLKATA)).toBe('2026-11-07T21:30:00+05:30');
	});

	it('uses the new offset just after a clock change, and Z for UTC itself', () => {
		expect(venueTime('2026-11-07', '23:00', { latE6: 51_480_000, lngE6: 0 })).toBe('2026-11-07T23:00:00Z');
		// Berlin's clocks go forward at 02:00 on 29 March 2026.
		expect(venueTime('2026-03-29', '03:30', BERLIN)).toBe('2026-03-29T03:30:00+02:00');
		expect(venueTime('2026-03-29', '01:30', BERLIN)).toBe('2026-03-29T01:30:00+01:00');
	});

	it('refuses dates and times that do not exist', () => {
		expect(() => venueTime('2026-02-30', '22:00', BERLIN)).toThrow(/valid date/);
		expect(() => venueTime('2026-11-07', '24:00', BERLIN)).toThrow(/valid date/);
	});
});

describe('atVenue', () => {
	it("reads any datetime on the venue's clock", () => {
		expect(atVenue('2026-11-07T22:00:00.000Z', BERLIN)).toBe('2026-11-07T23:00:00+01:00');
		expect(atVenue('2026-11-08T05:00:00+01:00', NEW_YORK)).toBe('2026-11-07T23:00:00-05:00');
		expect(atVenue('2026-11-07T22:00:00+01:00', { latE6: 51_480_000, lngE6: 0 })).toBe('2026-11-07T21:00:00Z');
	});
});
