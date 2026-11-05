<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import Pagination from './Pagination.svelte';

	/** The projects directory, filtered, so the links keep the search and only change the page. */
	const { Story } = defineMeta({
		title: 'Patterns/Pagination',
		component: Pagination,
		tags: ['autodocs'],
		parameters: { sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/projects?genre=techno&page=2') } } } }
	});

	const links = (canvasElement: HTMLElement) => {
		const canvas = within(canvasElement);

		return [canvas.queryByRole('link', { name: 'Previous' }), canvas.queryByRole('link', { name: 'Next' })].map((link) => link?.getAttribute('href') ?? null);
	};

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story
	name="First page"
	args={{ page: 1, pageSize: 20, total: 45 }}
	play={async ({ canvasElement }) => {
		await expect(links(canvasElement)).toEqual([null, '/projects?genre=techno&page=2']);
	}}
/>

<Story
	name="Middle page"
	args={{ page: 2, pageSize: 20, total: 45 }}
	play={async ({ canvasElement }) => {
		await expect(links(canvasElement)).toEqual(['/projects?genre=techno&page=1', '/projects?genre=techno&page=3']);
	}}
/>

<Story
	name="Last page"
	args={{ page: 3, pageSize: 20, total: 45 }}
	play={async ({ canvasElement }) => {
		await expect(links(canvasElement)).toEqual(['/projects?genre=techno&page=2', null]);
	}}
/>

<!-- Everything fits on one page, so there's nothing to page through. -->
<Story
	name="One page"
	args={{ page: 1, pageSize: 20, total: 12 }}
	play={async ({ canvasElement }) => {
		await expect(within(canvasElement).queryByRole('navigation', { name: 'Pagination' })).toBeNull();
	}}
/>

<Story name="Phone, middle page" args={{ page: 2, pageSize: 20, total: 45 }} globals={phone} />
