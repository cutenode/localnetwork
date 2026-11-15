import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import VenueSettings from './VenueSettings.svelte';

const blocks = [
	{ rkey: '3a', kind: 'party', name: 'Night Shift' },
	{ rkey: '3b', kind: 'profile', name: '@dj.test' }
];

describe('VenueSettings', () => {
	it('shows owners both sections, with the mode picked and each block to take away', async () => {
		await render(VenueSettings, { approval: 'approval', can: { edit: true, block: true }, blocks });

		await expect.element(page.getByRole('radio', { name: /Venue approves/ })).toBeChecked();
		await expect.element(page.getByRole('radio', { name: /Anyone can list/ })).not.toBeChecked();
		await expect.element(page.getByRole('button', { name: 'Unblock Night Shift' })).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Unblock @dj.test' })).toBeVisible();
		const nightShift = page.getByRole('listitem').filter({ hasText: 'Night Shift' });
		await expect.element(nightShift.getByText('Party', { exact: true })).toBeVisible();
	});

	it("leaves out what the viewer can't change, and says when nobody's blocked", async () => {
		await render(VenueSettings, { approval: 'open', can: { edit: false, block: true }, blocks: [] });

		await expect.element(page.getByRole('heading', { name: 'Events that name this venue' })).not.toBeInTheDocument();
		await expect.element(page.getByText("Nobody's blocked.")).toBeVisible();
	});

	it('shows someone who can only edit the approval mode, and nothing about blocks', async () => {
		await render(VenueSettings, { approval: 'open', can: { edit: true, block: false }, blocks: [] });

		await expect.element(page.getByRole('radio', { name: /Anyone can list/ })).toBeChecked();
		await expect.element(page.getByRole('heading', { name: 'Blocked' })).not.toBeInTheDocument();
	});

	it('refills the block form after a refusal', async () => {
		await render(VenueSettings, {
			approval: 'open',
			can: { edit: true, block: true },
			blocks: [],
			values: { kind: 'act', name: 'crew' }
		});

		await expect.element(page.getByRole('combobox', { name: 'Block a' })).toHaveValue('act');
		await expect.element(page.getByRole('textbox', { name: 'Handle or link' })).toHaveValue('crew');
		await expect.element(page.getByRole('heading', { name: 'Blocked' })).toBeVisible();
	});
});
