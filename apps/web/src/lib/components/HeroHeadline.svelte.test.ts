import { cdp, page } from 'vitest/browser';
import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import headline from '#lib/headline.json' with { type: 'json' };
import HeroHeadline from './HeroHeadline.svelte';
// What's showing is down to the site's classes, so the test draws with them.
import '../../routes/layout.css';

const { verbs } = headline;

const nouns = headline.nouns.map((noun) => `${noun}.`);

const isShowing = (el: Element) => el.checkVisibility({ visibilityProperty: true, opacityProperty: true });

// The drawn copies of these words that can be seen right now, old and new together mid-swap.
const showing = (words: string[]) =>
	words.flatMap((word) => page.getByText(word, { exact: true }).elements().filter(isShowing));

const words = (els: Element[]) => els.map((el) => el.textContent).join('|');

const show = (interval: number) => render(HeroHeadline, { interval });

const reducedMotion = (value: 'reduce' | 'no-preference') =>
	cdp().send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value }] });

afterEach(() => reducedMotion('no-preference'));

describe('HeroHeadline', () => {
	it('reads as the first pair', async () => {
		await show(2200);

		await expect.element(page.getByText(`${verbs[0]} your ${nouns[0]}`, { exact: true })).toBeInTheDocument();
		expect(words(showing(verbs))).toBe(verbs[0]);
		expect(words(showing(nouns))).toBe(nouns[0]);
	});

	it('swaps the verb and the noun on their own timers while "your" stays put', async () => {
		const interval = 300;
		await show(interval);
		const your = page.getByText('your', { exact: true }).element();
		const { left, top } = your.getBoundingClientRect();
		let last = [verbs[0], nouns[0]];
		let alone = false;
		const seen = [new Set<string>(), new Set<string>()];

		const sample = () => {
			const now = [words(showing(verbs)), words(showing(nouns))];
			const changed = now.map((slot, s) => slot !== last[s]);
			// One slot swapping while the other holds still, which a shared timer would never show.
			alone ||= changed[0] !== changed[1];
			last = now;

			now.forEach((slot, s) => slot.split('|').forEach((word) => seen[s].add(word)));

			return alone && seen.every((slot) => slot.size > 1);
		};

		await expect.poll(sample, { interval: 16, timeout: interval * 10 }).toBe(true);
		expect(your.getBoundingClientRect()).toMatchObject({ left, top });
	});

	it('brings new words in from above and from below', async () => {
		const { container } = await show(250);
		const slots = [...container.querySelectorAll('[aria-hidden="true"] > .inline-grid')];
		const met = new WeakSet<Element>();
		const from = new Set<'above' | 'below'>();

		// A word first met off its resting line is an incoming one, still where it came from.
		await expect
			.poll(
				() => {
					for (const slot of slots) {
						const home = slot.getBoundingClientRect().top;

						for (const word of slot.querySelectorAll(':scope > :not(.invisible)')) {
							if (met.has(word)) continue;
							met.add(word);

							const offset = word.getBoundingClientRect().top - home;

							if (Math.abs(offset) >= 1) from.add(offset < 0 ? 'above' : 'below');
						}
					}

					return from.size;
				},
				{ interval: 16, timeout: 10_000 }
			)
			.toBe(2);
	});

	it('stays on the first pair with reduced motion', async () => {
		await reducedMotion('reduce');
		const interval = 100;
		await show(interval);

		// Moving, both slots would have swapped by now.
		await new Promise((resolve) => setTimeout(resolve, interval * 4));
		expect(words(showing(verbs))).toBe(verbs[0]);
		expect(words(showing(nouns))).toBe(nouns[0]);
	});
});
