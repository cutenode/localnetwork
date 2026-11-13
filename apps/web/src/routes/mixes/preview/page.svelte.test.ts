import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Page from './+page.svelte';

it('loads the sample report when asked, and closes it again', async () => {
	await render(Page);

	await page.getByRole('button', { name: 'See a sample' }).click();
	await expect.element(page.getByRole('heading', { name: 'Sunday Warm-up' })).toBeVisible();

	await page.getByRole('button', { name: 'Close' }).click();
	await expect.element(page.getByRole('heading', { name: 'Preview a mix report' })).toBeVisible();
});
