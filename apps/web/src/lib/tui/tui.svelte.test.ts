import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Cast from './Cast.svelte';
import { capture } from './captures.ts';

it('swaps one recording for another in a single player', async () => {
	const [first, second] = await Promise.all([capture('beat/two-bars', 'dark'), capture('beat/two-bars', 'light')]);
	const shown = await render(Cast, { cast: first.bytes });
	const players = () => shown.container.querySelectorAll('.ap-player');

	await expect.poll(() => players().length).toBe(1);
	await shown.rerender({ cast: second.bytes });

	await expect.poll(() => players().length).toBe(1);
});
