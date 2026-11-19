import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import ForCrumbs from './ForCrumbs.svelte';

const crumbs = () => page.getByRole('navigation', { name: 'Breadcrumb' });

describe('the /for breadcrumb', () => {
	it.each([
		['djs', 'DJs', '/for/djs', 'For promoters', '/for/promoters'],
		['promoters', 'Promoters', '/for/promoters', 'For DJs', '/for/djs']
	] as const)(
		'on the %s page, leads back to For and across to the other side',
		async (side, label, href, other, otherHref) => {
			await render(ForCrumbs, { side });

			await expect.element(crumbs().getByRole('link', { name: 'For', exact: true })).toHaveAttribute('href', '/for');
			await expect.element(crumbs().getByRole('link', { name: label })).toHaveAttribute('aria-current', 'page');
			await expect.element(crumbs().getByRole('link', { name: label })).toHaveAttribute('href', href);
			await expect.element(page.getByRole('link', { name: other })).toHaveAttribute('href', otherHref);
		}
	);

	it('names the current side and the other one so the browser morphs one into the other between pages', async () => {
		await render(ForCrumbs, { side: 'djs' });

		expect(crumbs().getByRole('link', { name: 'DJs' }).element().style.viewTransitionName).toBe('side-djs');
		expect(page.getByRole('link', { name: 'For promoters' }).element().style.viewTransitionName).toBe('side-promoters');
	});
});
