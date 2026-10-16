<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Badge from './Badge.svelte';
	import Tag from './Tag.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Tag and badge', component: Tag, tags: ['autodocs'] });
</script>

<script lang="ts">
	let filter = $state('All');
</script>

<Story name="Genres" asChild>
	<div class="flex flex-wrap gap-1.5"><Tag>techno</Tag><Tag>dub techno</Tag><Tag>hypnotic techno</Tag></div>
</Story>

<Story name="Linked genres" asChild>
	<div class="flex flex-wrap gap-1.5"><Tag href="/projects?genres=disco">disco</Tag><Tag href="/projects?genres=house">house</Tag><Tag href="/projects?genres=edits">edits</Tag></div>
</Story>

<Story name="Filters" asChild>
	<div role="group" aria-label="Kind of night" class="flex flex-wrap gap-1.5">
		{#each ['All', 'Club', 'Radio', 'Daytime'] as option (option)}<Tag pressed={filter === option} onclick={() => (filter = option)}>{option}</Tag>{/each}
	</div>
</Story>

<Story name="Mono" asChild>
	<div class="flex flex-wrap gap-1.5"><Tag variant="mono">club</Tag><Tag variant="mono">radio</Tag><Tag variant="mono">livestream</Tag></div>
</Story>

<Story name="Badge" asChild>
	<Badge label="New" href="/mixes">Mixes now show every track and transition</Badge>
</Story>
