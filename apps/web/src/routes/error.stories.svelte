<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import ErrorPage from './+error.svelte';

	/** The site's error page, for a page that isn't there and for a fault. Only a fault shows a reference to report. */
	const { Story } = defineMeta({ title: 'Patterns/Error page', component: ErrorPage, parameters: { layout: 'fullscreen' } });

	const at = (status: number, message: string) => ({
		sveltekit_experimental: {
			state: { page: { url: new URL('https://localnetwork.computer/projects/nobody'), status, error: { message, requestId: 'req-3f2a' } } }
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
