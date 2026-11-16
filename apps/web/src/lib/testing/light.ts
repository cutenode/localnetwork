/**
 * A story's `beforeEach` for the light palette (packages/tokens/theme.css), which applies under a `light` class on the
 * root, so the story's accessibility check measures contrast in it too.
 */
export const light = () => {
	document.documentElement.classList.add('light');

	return () => document.documentElement.classList.remove('light');
};
