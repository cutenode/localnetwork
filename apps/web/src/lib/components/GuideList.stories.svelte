<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { findTool } from '#lib/tools/index.ts';
	import GuideList from './GuideList.svelte';

	// As the tool pages' load lists them: localtoolbox's real guides.
	const guides = (findTool('localtoolbox')?.docs ?? []).map(({ slug, title }) => ({ slug, title }));

	/** A tool's guides, beside the page on a wide screen and folded into a button above it on a phone. */
	const { Story } = defineMeta({
		title: 'Patterns/Guide list',
		component: GuideList,
		tags: ['autodocs'],
		args: { tool: 'localtoolbox', guides, current: guides[1]?.slug }
	});

	const desktop = { viewport: { value: 'desktop', isRotated: false } };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Beside the page" globals={desktop} />

<!-- On the tool's own page, with no guide open. -->
<Story name="No guide open" args={{ current: undefined }} globals={desktop} />

<Story name="Folded on a phone" globals={phone} />

<Story
	name="Unfolded on a phone"
	globals={phone}
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);

		await userEvent.click(canvas.getByRole('button', { name: /^Guides/ }));
		await expect(canvas.getByRole('link', { name: guides[0].title })).toBeVisible();
	}}
/>
