import { cdp, page } from 'vitest/browser';
import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import headline from '#lib/headline.json' with { type: 'json' };
import HeroHeadline from './HeroHeadline.svelte';
// What's showing is down to the site's classes, so the test draws with them.
import '../../routes/layout.css';

const { verbs, nouns } = headline;

const steps = Math.max(verbs.length, nouns.length);

const phrase = (i: number) => `${verbs[i % verbs.length]} your ${nouns[i % nouns.length]}.`;

// The drawn phrase, not the first pair's copy for screen readers, which comes before it.
const drawn = (i: number) => page.getByText(phrase(i), { exact: true }).last();

const show = (interval: number) => render(HeroHeadline, { interval });

const reducedMotion = (value: 'reduce' | 'no-preference') =>
	cdp().send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value }] });

afterEach(() => reducedMotion('no-preference'));

describe('HeroHeadline', () => {
	it('steps through every pair, then starts over', async () => {
		const interval = 300;
		await show(interval);

		await expect.element(drawn(0)).toBeVisible();
		await expect.element(drawn(1)).toBeVisible();
		await expect.element(drawn(steps - 1), { timeout: steps * interval + 1000 }).toBeVisible();
		await expect.element(drawn(0), { timeout: interval + 1000 }).toBeVisible();
		await expect.element(drawn(1), { timeout: interval + 1000 }).toBeVisible();
	});

	it('stays on the first pair with reduced motion', async () => {
		await reducedMotion('reduce');
		const interval = 100;
		await show(interval);

		// Moving, the first pair would have faded out by now and the cycle wouldn't be over yet.
		await new Promise((resolve) => setTimeout(resolve, interval * 4));
		expect(drawn(0).element()).toBeVisible();
		expect(drawn(2).element()).not.toBeVisible();
	});
});
