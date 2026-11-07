import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import DiffTable from './DiffTable.svelte';
// The table only runs past its box with the site's CSS on it.
import '../../routes/layout.css';

describe('DiffTable', () => {
	it('says when it runs past the edge, and takes a fade away at the end', async () => {
		await page.viewport(320, 640);
		const after = 'Late-night jungle and a long run of records nobody else in the city owns on vinyl';
		await render(DiffTable, { changes: [{ field: 'profile.bookingAndTravelPreferences.description', after }] });

		const box = page.getByRole('region', { name: 'Changes' });
		await expect.element(page.getByText('Scroll sideways to see it all')).toBeVisible();
		await expect.element(box).toHaveAttribute('tabindex', '0');
		await expect.element(box).toHaveClass(/mask-r-from/);

		// Scrolled once the fonts are in, so the end it scrolls to is where the table ends.
		await document.fonts.ready;
		box.element().scrollLeft = box.element().scrollWidth;

		await expect.element(box).not.toHaveClass(/mask-r-from/);
		await expect.element(page.getByText('Scroll sideways to see it all')).toBeVisible();
		await page.viewport(414, 896);
	});
	it('stays quiet when everything fits', async () => {
		await render(DiffTable, { changes: [{ field: 'name', before: 'A', after: 'B' }] });

		await expect.element(page.getByRole('region', { name: 'Changes' })).not.toHaveAttribute('tabindex');
		await expect.element(page.getByText('Scroll sideways to see it all')).not.toBeInTheDocument();
	});
});
