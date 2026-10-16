<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Card from './Card.svelte';
	import Monogram from './Monogram.svelte';

	const { Story } = defineMeta({
		title: 'Primitives/Card',
		component: Card,
		tags: ['autodocs'],
		argTypes: {
			tone: { control: 'inline-radio', options: ['surface', 'outline', 'well'] },
			padding: { control: 'inline-radio', options: ['none', 'sm', 'md', 'lg'] },
			radius: { control: 'inline-radio', options: ['lg', 'xl', '2xl'] }
		},
		args: { tone: 'surface', padding: 'lg', radius: '2xl' }
	});
</script>

<Story name="Panel">
	{#snippet template(args)}
		<Card tone={args.tone} padding={args.padding} radius={args.radius} class="flex max-w-90 flex-col gap-2">
			<h3 class="m-0 text-emphasis font-semibold">Private inquiries</h3>
			<p class="m-0 text-fg-muted">Each booking is its own thread, shared only with the project's managers, with your show details up front.</p>
		</Card>
	{/snippet}
</Story>

<Story name="Linked" asChild>
	<Card href="/projects/lowkey-lagos" padding="md" class="flex max-w-80 flex-col gap-3">
		<span class="flex items-center gap-3"><Monogram name="Lowkey Lagos" size="lg" /><span><span class="block text-emphasis font-semibold">Lowkey Lagos</span><span class="block text-label text-fg-muted">Collective · London</span></span></span>
		<span class="text-small text-fg-soft">Amapiano and afro house crew</span>
	</Card>
</Story>

<Story name="Tones" asChild>
	<div class="flex flex-wrap gap-4">
		{#each ['surface', 'outline', 'well'] as const as tone (tone)}<Card {tone} class="w-48"><span class="font-mono text-caption">{tone}</span></Card>{/each}
	</div>
</Story>
