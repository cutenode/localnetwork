import { describe, expect, it } from 'vitest';
import { RateLimiter } from './rate-limit';

describe('RateLimiter', () => {
	it('allows bursts up to capacity, then refills over time', () => {
		let now = 0;
		const limiter = new RateLimiter(3, 1, () => now);
		expect([limiter.take('a'), limiter.take('a'), limiter.take('a'), limiter.take('a')]).toEqual([
			true,
			true,
			true,
			false
		]);
		expect(limiter.take('b')).toBe(true);
		now = 1500;
		expect(limiter.take('a')).toBe(true);
		expect(limiter.take('a')).toBe(false);
		now = 10_000;
		expect(limiter.take('a', 3)).toBe(true);
	});

	it('says whether a key has a token left without taking it', () => {
		let now = 0;
		const limiter = new RateLimiter(1, 1, () => now);

		expect([limiter.has('a'), limiter.has('a')]).toEqual([true, true]);
		limiter.take('a');
		expect(limiter.has('a')).toBe(false);
		now = 1000;
		expect(limiter.has('a')).toBe(true);
	});
});
