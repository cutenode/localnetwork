import { cdp, page, userEvent } from 'vitest/browser';
import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { sampleReport } from '#lib/mix/sample.ts';
import MixReport from './MixReport.svelte';
import MixTimeline from './MixTimeline.svelte';
// The lane sizes and the reduced-motion rules come from the site's stylesheet, so the tests draw with it.
import '../../../routes/layout.css';

const reducedMotion = (value: 'reduce' | 'no-preference') =>
	cdp().send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value }] });

const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));

afterEach(async () => {
	await reducedMotion('no-preference');
	await page.viewport(414, 896);
});

describe('the mix report on a phone', () => {
	it('gives each timeline lane a target a thumb can hit', async () => {
		await page.viewport(390, 844);
		await render(MixTimeline, { report: sampleReport, selected: null });

		const lane = page.getByRole('button', { name: /^1 Night Bus/ }).element();

		expect(lane.getBoundingClientRect().height).toBeGreaterThanOrEqual(24);
	});
});

describe('with reduced motion', () => {
	it('jumps to a picked track instead of scrolling there', async () => {
		await reducedMotion('reduce');
		await page.viewport(390, 500);
		await render(MixReport, { report: sampleReport });
		const last = sampleReport.plays.length;
		const lane = page.getByRole('button', { name: new RegExp(`^${last} `) }).first();

		await userEvent.click(lane);
		await frame();
		const now = window.scrollY;
		await new Promise((resolve) => setTimeout(resolve, 600));

		expect(now).toBeGreaterThan(0);
		expect(window.scrollY).toBe(now);
	});

	it('holds loading placeholders still', async () => {
		const placeholder = document.createElement('div');
		placeholder.className = 'animate-pulse';
		document.body.append(placeholder);

		expect(getComputedStyle(placeholder).animationName).not.toBe('none');
		await reducedMotion('reduce');
		expect(getComputedStyle(placeholder).animationName).toBe('none');

		placeholder.remove();
	});
});
