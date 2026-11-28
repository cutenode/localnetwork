<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { phone } from '#lib/testing/viewports.ts';
	import { tools } from '#lib/tools/index.ts';
	import WithGuides from './WithGuides.svelte';

	/** A tool page or guide beside the tool's guides, which fold into a button above the page on a phone. */
	const { Story } = defineMeta({ title: 'Patterns/Tool guides', component: WithGuides, tags: ['autodocs'], parameters: { layout: 'fullscreen' } });

	const { id, docs } = tools[1];

	const guides = docs.map(({ slug, title }) => ({ slug, title }));

	const current = guides[1];

	/** The guide being read is marked in the list. */
	const marksCurrent = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await expect(within(canvasElement).getByRole('link', { name: current.title })).toHaveAttribute('aria-current', 'page');
	};
</script>

{#snippet page()}
	<section class="flex flex-col gap-4">
		<h2 class="text-title">Downloads and gates</h2>
		<p class="text-lede text-fg-soft">Nothing goes on the list by itself. Open a SoundCloud playlist, Likes page or track page, scroll a long list to the end, then open the Manager.</p>
	</section>
{/snippet}

{#snippet install()}
	<section class="flex flex-col gap-4">
		<h2 class="text-title">Install</h2>
		<p class="text-lede text-fg-soft">Add it to Chrome, then open SoundCloud.</p>
	</section>
{/snippet}

<!-- desktop-only: the guide list folds away below lg; the Guide list stories open it on a phone. -->
<Story name="Guide" asChild play={marksCurrent} tags={['desktop-only']}>
	<div class="container-page py-10"><WithGuides tool={id} {guides} current={current.slug}>{@render page()}</WithGuides></div>
</Story>

<Story name="Tool page, with a lead" asChild>
	<div class="container-page py-10"><WithGuides tool={id} {guides} lead={install}>{@render page()}</WithGuides></div>
</Story>

<Story name="Phone" asChild globals={phone}>
	<div class="container-page py-10"><WithGuides tool={id} {guides} current={current.slug}>{@render page()}</WithGuides></div>
</Story>

<Story name="Phone, with a lead" asChild globals={phone}>
	<div class="container-page py-10"><WithGuides tool={id} {guides} lead={install}>{@render page()}</WithGuides></div>
</Story>
