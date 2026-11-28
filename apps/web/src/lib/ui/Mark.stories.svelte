<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Mark from './Mark.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Mark', component: Mark, tags: ['autodocs'] });
</script>

<Story name="Sizes" asChild>
	<div class="flex items-end gap-6"><Mark class="h-4" /><Mark /><Mark class="h-8" /><Mark class="h-16" /></div>
</Story>

<!-- The site header's lockup: the mark is decoration, so the name beside it is what's read out. -->
<Story name="With the name" asChild>
	<p class="flex items-center gap-2.5 text-emphasis text-fg"><Mark />Local Network</p>
</Story>
