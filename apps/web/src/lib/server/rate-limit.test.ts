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

	it('gives back a token it took, up to capacity', () => {
		const limiter = new RateLimiter(1, 0);

		expect(limiter.take('a')).toBe(true);
		expect(limiter.take('a')).toBe(false);
		limiter.refund('a');
		limiter.refund('a');
		expect([limiter.take('a'), limiter.take('a')]).toEqual([true, false]);
	});
});
