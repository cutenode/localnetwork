import { page, userEvent } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { ACCESS_PRESETS } from '#lib/epk.ts';
import AccessMatrix from './AccessMatrix.svelte';
// The rules per group show through CSS alone, so a pick made before the page hydrates still opens them.
import '../../../routes/layout.css';

describe('AccessMatrix', () => {
	it('offers the common presets, and the rules per group only for your own mix', async () => {
		await render(AccessMatrix, { access: ACCESS_PRESETS[1].access, preset: ACCESS_PRESETS[1].id });

		await expect.element(page.getByRole('radio', { name: /Promoters automatically/ })).toBeChecked();
		await expect.element(page.getByRole('radio', { name: /^Open/ })).not.toBeInTheDocument();
		await expect.element(page.getByRole('radio', { name: 'Anyone signed in: You approve' })).not.toBeInTheDocument();

		await userEvent.click(page.getByRole('radio', { name: /Your own mix/ }));
		await userEvent.click(page.getByRole('radio', { name: 'Anyone signed in: Approved automatically' }));
		await expect.element(page.getByRole('radio', { name: /Your own mix/ })).toBeChecked();

		await userEvent.click(page.getByRole('radio', { name: /Nobody can ask/ }));
		await expect
			.element(page.getByRole('radio', { name: "People you've booked with: Can't ask" }))
			.not.toBeInTheDocument();
		await userEvent.click(page.getByRole('radio', { name: /Your own mix/ }));
		await expect.element(page.getByRole('radio', { name: "People you've booked with: Can't ask" })).toBeChecked();
	});

	it("keeps a press kit's uncommon preset on offer", async () => {
		await render(AccessMatrix, { access: ACCESS_PRESETS[0].access, preset: ACCESS_PRESETS[0].id });

		await expect.element(page.getByRole('radio', { name: /^Open/ })).toBeChecked();
	});

	it('gives each rule a cell-sized target', async () => {
		await render(AccessMatrix, { access: ACCESS_PRESETS[1].access, preset: 'custom' });

		const cell = page.getByRole('radio', { name: "Promoters: Can't ask" }).element().closest('label');

		expect(cell?.getBoundingClientRect().height).toBeGreaterThanOrEqual(44);
	});

	it('keeps the rules on screen when the owner picks their own mix', async () => {
		await render(AccessMatrix, { access: ACCESS_PRESETS[1].access, preset: ACCESS_PRESETS[1].id });

		await userEvent.click(page.getByRole('radio', { name: /Your own mix/ }));

		await expect.element(page.getByRole('radio', { name: /Your own mix/ })).toBeChecked();
		await expect.element(page.getByRole('radio', { name: 'Promoters: Approved automatically' })).toBeChecked();
		await expect.element(page.getByRole('radio', { name: 'Anyone signed in: You approve' })).toBeChecked();
	});
});
