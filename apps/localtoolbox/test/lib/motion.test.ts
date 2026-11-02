// src/lib/motion.ts — the transitions' parameters, under reduced motion.
import { describe, it, expect, vi } from 'vitest';
import { motion } from '../../src/lib/motion.ts';

/** A browser that answers the reduced-motion query with `reduce`. */
const asking = (reduce: boolean) =>
	vi.stubGlobal('matchMedia', (query: string) => ({
		matches: reduce && query === '(prefers-reduced-motion: reduce)'
	}));

describe('motion', () => {
	it('plays a transition as written when nothing asks for less', () => {
		asking(false);
		expect(motion({ y: 240, duration: 240, opacity: 1 })).toEqual({ y: 240, duration: 240, opacity: 1 });
	});

	it('takes the time out of it, delay and all, when the browser asks for less motion', () => {
		asking(true);
		expect(motion({ y: 240, duration: 240, delay: 110 })).toEqual({ y: 240, duration: 0, delay: 0 });
	});

	it('reads the setting as each transition starts, so changing it mid-session counts', () => {
		asking(false);
		expect(motion({ duration: 150 }).duration).toBe(150);
		asking(true);
		expect(motion({ duration: 150 }).duration).toBe(0);
	});

	it('plays as written where there is no media query to ask', () => {
		expect(globalThis.matchMedia).toBeUndefined();
		expect(motion({ duration: 180 })).toEqual({ duration: 180 });
	});
});
