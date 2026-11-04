import * as fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { niceTicks } from './report.ts';

describe('niceTicks', () => {
	it('labels padded BPM ranges as the tempo chart always has', () => {
		expect(niceTicks(120, 134, 4)).toEqual([120, 125, 130]);
		expect(niceTicks(86.5, 92.25, 4)).toEqual([88, 90, 92]);
		expect(niceTicks(123.88, 128.12, 4)).toEqual([124, 126, 128]);
		expect(niceTicks(170, 176, 4)).toEqual([170, 172, 174, 176]);
		expect(niceTicks(60, 180, 4)).toEqual([100, 150]);
	});

	it('keeps the tick a pitched BPM lands a rounding error short of', () => {
		expect(niceTicks(110, 129.99999999999997, 4)).toEqual([110, 115, 120, 125, 130]);
	});

	it('gives an empty range its one value', () => {
		expect(niceTicks(128, 128, 4)).toEqual([128]);
	});

	it('puts a tick in a span narrower than the first step that fits one gap', () => {
		expect(niceTicks(5e-324, 0.0010000000000000002, 1)).toEqual([0, 0.001]);
	});

	// Counts of 3 and up get at least one from the bound below.
	it('always puts a tick in the range, even for a count of 1 or 2', () => {
		fc.assert(
			fc.property(chartRange(fc.integer({ min: 1, max: 2 })), ({ low, high, count }) => {
				expect(niceTicks(low, high, count).length).toBeGreaterThan(0);
			})
		);
	});

	it('keeps ticks within the range, up to rounding', () => {
		fc.assert(
			fc.property(chartRange(anyCount), ({ low, high, count }) => {
				const slack = (high - low) * 1e-9;

				for (const tick of niceTicks(low, high, count)) {
					expect(tick).toBeGreaterThanOrEqual(low - slack);
					expect(tick).toBeLessThanOrEqual(high + slack);
				}
			})
		);
	});

	it('steps through consecutive multiples of 1, 2 or 5 times a power of ten', () => {
		fc.assert(
			fc.property(
				chartRange(anyCount).filter(({ low, high, count }) => niceTicks(low, high, count).length > 1),
				({ low, high, count }) => {
					const ticks = niceTicks(low, high, count);
					const step = Number((ticks[1] - ticks[0]).toPrecision(1));

					expect(step.toExponential()).toMatch(/^[125]e/);
					ticks.forEach((tick, i) => {
						const multiple = Math.round(tick / step);

						expect(multiple).toBe(Math.round(ticks[0] / step) + i);
						expect(Math.abs(tick - multiple * step)).toBeLessThanOrEqual(4 * Number.EPSILON * (Math.abs(tick) + step));
					});
				}
			)
		);
	});

	// No more than count + 1, or 3 when a count under 3 has to step down to fit a tick in. The step is under
	// 2.5 * span / count, so at least count / 2.5 of them.
	it('gives about `count` ticks', () => {
		fc.assert(
			fc.property(chartRange(anyCount), ({ low, high, count }) => {
				const ticks = niceTicks(low, high, count);

				expect(ticks.length).toBeLessThanOrEqual(Math.max(count + 1, 3));
				expect(ticks.length).toBeGreaterThanOrEqual(Math.floor(count / 2.5));
			})
		);
	});
});

// Helpers

// Ranges a chart could have. Ticks stay exact multiples of the step while value / step stays well under 2 ** 53.
function chartRange(counts: fc.Arbitrary<number>) {
	return fc
		.record({
			low: fc.double({ min: -1e6, max: 1e6, noNaN: true }),
			width: fc.double({ min: 1e-6, max: 1e6, noNaN: true }),
			count: counts
		})
		.map(({ low, width, count }) => ({ low, high: low + width, count }));
}

const anyCount = fc.integer({ min: 1, max: 100 });
