// Svelte's transitions run on the Web Animations API, which the stylesheet's
// prefers-reduced-motion rule never reaches: it shortens CSS animations and
// transitions only. So every transition's parameters go through here, and
// come back with no duration when the browser asks for less motion.

/** Whether the browser asks for less motion, read at the moment a transition starts. */
const reduced = () => globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;

/** A transition's parameters, with `duration` and `delay` at 0 under reduced motion. */
export function motion<T extends { duration?: number; delay?: number }>(params: T): T {
	return reduced() ? { ...params, duration: 0, delay: 0 } : params;
}
