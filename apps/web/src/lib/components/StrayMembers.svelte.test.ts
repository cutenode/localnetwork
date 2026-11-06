import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import StrayMembers from './StrayMembers.svelte';

describe('StrayMembers', () => {
	it('names who is left and where', async () => {
		await render(StrayMembers, {
			strays: [{ did: 'did:plc:mod', handle: 'mod.test', spaces: ['audit', 'industry'] }]
		});

		await expect.element(page.getByRole('heading', { name: 'Still in private spaces' })).toBeVisible();
		await expect.element(page.getByRole('link', { name: '@mod.test' })).toBeVisible();
		await expect.element(page.getByText('audit, industry')).toBeVisible();
	});

	it('shows nothing when nobody is left', async () => {
		await render(StrayMembers, { strays: [] });

		await expect.element(page.getByRole('heading')).not.toBeInTheDocument();
	});

	it("says when it couldn't check", async () => {
		await render(StrayMembers, { strays: null });

		await expect
			.element(page.getByRole('alert'))
			.toHaveTextContent("Couldn't check the private spaces. Reload the page to try again.");
	});
});
