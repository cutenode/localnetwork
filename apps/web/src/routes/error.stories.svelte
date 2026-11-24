<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import ErrorPage from './+error.svelte';
	import { phone } from '#lib/testing/viewports.ts';

	/**
	 * The site's error page, for a page that isn't there, an account that isn't active, and a fault. Only a fault shows a
	 * reference to report.
	 */
	const { Story } = defineMeta({ title: 'Patterns/Error page', component: ErrorPage, parameters: { layout: 'fullscreen' } });

	const at = (status: number, message: string, title?: string) => ({
		sveltekit_experimental: {
			state: { page: { url: new URL('https://localnetwork.computer/projects/nobody'), status, error: { message, title, requestId: 'req-3f2a' } } }
		}
	});
</script>

<Story
	name="Not found"
	parameters={at(404, 'Not Found')}
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole('heading', { level: 1, name: "We couldn't find that page" })).toBeVisible();
		await expect(canvas.queryByText(/Reference/)).toBeNull();
	}}
/>

<Story
	name="Something went wrong"
	parameters={at(500, 'Internal Error')}
	play={async ({ canvasElement }) => {
		await expect(within(canvasElement).getByText('Reference: req-3f2a')).toBeVisible();
	}}
/>

<Story
	name="Account isn't active"
	parameters={at(404, 'Their profile and sets come back when the account does.', "@mara.test isn't active")}
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole('heading', { level: 1, name: "@mara.test isn't active" })).toBeVisible();
		await expect(canvas.queryByText('Error 404')).toBeNull();
	}}
/>

<Story
	name="Account deleted"
	parameters={at(410, 'Their profile and sets are gone.', '@mara.test was deleted')}
	play={async ({ canvasElement }) => {
		await expect(within(canvasElement).getByRole('heading', { level: 1, name: '@mara.test was deleted' })).toBeVisible();
	}}
/>

<!-- A handle has few places to break, so a long one has to wrap rather than push the page sideways on a phone. -->
<Story
	name="Account isn't active, long handle, on a phone"
	globals={phone}
	parameters={at(404, 'Their profile and sets come back when the account does.', "@djmarathonsoundsystem.berlin isn't active")}
	play={async ({ canvasElement }) => {
		await expect(canvasElement.ownerDocument.documentElement.scrollWidth).toBeLessThanOrEqual(canvasElement.ownerDocument.documentElement.clientWidth);
	}}
/>

<!-- On a phone the actions fill the page's width, however short the heading above them is. -->
<Story
	name="Account deleted, on a phone"
	globals={phone}
	parameters={at(410, 'Their profile and sets are gone.', '@mara.test was deleted')}
	play={async ({ canvasElement }) => {
		const page = canvasElement.querySelector('.container-page');
		const home = within(canvasElement).getByRole('link', { name: 'Back to home' });
		const gutters = page ? Number.parseFloat(getComputedStyle(page).paddingLeft) * 2 : 0;

		await expect(home.getBoundingClientRect().width).toBe((page?.clientWidth ?? 0) - gutters);
	}}
/>
