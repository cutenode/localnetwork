<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import DateBlock from './DateBlock.svelte';
	import Monogram from './Monogram.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Monogram and date', component: Monogram, tags: ['autodocs'] });

	const names = ['Neon Tide', 'Saffron Social', 'Lowkey Lagos', 'Pirate Signal', 'Velvet Static', 'Sundial Soundsystem', 'Footwork Fever', 'Moss & Marrow', 'Harbour Lights', 'Coyote Cumbia Club', 'Glasshouse', 'Halflight'];
</script>

<Story name="Monograms" asChild>
	<ul class="m-0 grid max-w-160 list-none grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-3 p-0">
		{#each names as name (name)}<li class="flex items-center gap-3"><Monogram {name} /><span class="text-small">{name}</span></li>{/each}
	</ul>
</Story>

<Story name="Monogram sizes" asChild>
	<div class="flex items-end gap-3"><Monogram name="Neon Tide" size="sm" /><Monogram name="Neon Tide" /><Monogram name="Neon Tide" size="lg" /><Monogram name="Neon Tide" size="xl" /></div>
</Story>

<Story name="Dates" asChild>
	<div class="flex gap-3"><DateBlock date="2026-10-16" /><DateBlock date="2026-10-17" /><DateBlock date="2026-10-18" tone="aqua" /></div>
</Story>
