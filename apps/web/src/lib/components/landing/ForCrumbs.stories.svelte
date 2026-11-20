<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { light } from '#lib/testing/light.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import ForCrumbs from './ForCrumbs.svelte';

	/** The bar under the header on /for/djs and /for/promoters, marking its own side and linking to the other. */
	const { Story } = defineMeta({ title: 'Patterns/For breadcrumb', component: ForCrumbs, tags: ['autodocs'], parameters: { layout: 'fullscreen' } });

	/** Leads back to For, marks this side's page as the current one, and links across to the other side. */
	const marks = (label: string, href: string, other: string, otherHref: string) =>
		async ({ canvasElement }: { canvasElement: HTMLElement }) => {
			const canvas = within(canvasElement);
			const crumbs = within(canvas.getByRole('navigation', { name: 'Breadcrumb' }));

			await expect(crumbs.getByRole('link', { name: 'For' })).toHaveAttribute('href', '/for');
			await expect(crumbs.getByRole('link', { name: label })).toHaveAttribute('aria-current', 'page');
			await expect(crumbs.getByRole('link', { name: label })).toHaveAttribute('href', href);
			await expect(canvas.getByRole('link', { name: other })).toHaveAttribute('href', otherHref);
			await expect(canvas.getByRole('link', { name: other })).not.toHaveAttribute('aria-current');
		};

	/** Still one 52px row on a phone, with nothing pushed past its edge, so the page under it never moves. */
	const oneRow = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const row = within(canvasElement).getByRole('navigation', { name: 'Breadcrumb' }).parentElement;

		await expect(row?.getBoundingClientRect().height).toBe(52);
		await expect(row?.scrollWidth).toBeLessThanOrEqual(row?.clientWidth ?? 0);
	};
</script>

<Story name="On the DJs page" args={{ side: 'djs' }} play={marks('DJs', '/for/djs', 'For promoters', '/for/promoters')} />

<Story name="On the promoters page" args={{ side: 'promoters' }} play={marks('Promoters', '/for/promoters', 'For DJs', '/for/djs')} />

<Story name="On the DJs page, light" args={{ side: 'djs' }} beforeEach={light} />

<Story name="On the promoters page, light" args={{ side: 'promoters' }} beforeEach={light} />

<Story name="On the DJs page on a phone" args={{ side: 'djs' }} globals={phone} play={oneRow} />

<Story name="On the promoters page on a phone" args={{ side: 'promoters' }} globals={phone} play={oneRow} />
