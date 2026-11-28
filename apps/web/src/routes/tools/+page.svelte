<script lang="ts">
	import HotCues from '#lib/components/tools/HotCues.svelte';
	import ToolboxMenu from '#lib/components/tools/ToolboxMenu.svelte';
	import Button from '#lib/ui/Button.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<div class="container-page flex flex-col gap-20 py-10 sm:gap-28 sm:py-14">
	<div class="flex flex-col gap-4">
		<h1 class="text-display text-balance">From the crate to the booth.</h1>
		<p class="max-w-2xl text-intro text-fg-muted">Two free tools for DJs.</p>
	</div>
	<!-- Each tool on its own, the way it works: hot cues on a track, or its own menu over SoundCloud. -->
	{#each data.tools as tool (tool.id)}
		<section id={tool.id} aria-labelledby="{tool.id}-title" class="flex scroll-mt-6 flex-col gap-8 sm:gap-10">
			<SectionHeader id="{tool.id}-title" align="split" title={tool.name} lede={tool.pitch}>
				{#snippet actions()}
					<Button href="/tools/{tool.id}#install" aria-label="Install {tool.name}">Install</Button>
					<Button variant="secondary" href={tool.guide} aria-label="Guides for {tool.name}">Guides</Button>
				{/snippet}
			</SectionHeader>
			{#if tool.look === 'soundcloud'}
				<ToolboxMenu cues={tool.cues} />
			{:else}
				<HotCues tool={tool.name} cues={tool.cues} track={data.track} plain />
			{/if}
		</section>
	{/each}
</div>
