/** A per-key token bucket held in memory (the app runs as a single process). */
export class RateLimiter {
	private readonly buckets = new Map<string, { tokens: number; updated: number }>();

	constructor(
		private readonly capacity: number,
		private readonly refillPerSecond: number,
		private readonly now: () => number = Date.now
	) {}

	/** Takes one token; returns false when the key is out of tokens. */
	take(key: string, cost = 1): boolean {
		const bucket = this.refill(key);

		if (bucket.tokens < cost) return false;

		bucket.tokens -= cost;

		return true;
	}

	/**
	 * Gives back tokens a take charged for work that turned out not to count (like a lookup that found something), up
	 * to capacity. Charging first and refunding after keeps requests made together from all getting through before any
	 * of them is charged.
	 */
	refund(key: string, amount = 1): void {
		const bucket = this.refill(key);
		bucket.tokens = Math.min(this.capacity, bucket.tokens + amount);
	}

	/** The key's bucket, topped up for the time since it was last used. */
	private refill(key: string) {
		const now = this.now();
		const bucket = this.buckets.get(key) ?? { tokens: this.capacity, updated: now };
		const elapsed = Math.max(0, now - bucket.updated) / 1000;
		bucket.tokens = Math.min(this.capacity, bucket.tokens + elapsed * this.refillPerSecond);
		bucket.updated = now;

		if (this.buckets.size > 50_000) this.prune(now);
		this.buckets.set(key, bucket);

		return bucket;
	}

	private prune(now: number): void {
		const full = this.capacity / this.refillPerSecond;

		for (const [key, bucket] of this.buckets) {
			if ((now - bucket.updated) / 1000 > full) this.buckets.delete(key);
		}
	}
}
