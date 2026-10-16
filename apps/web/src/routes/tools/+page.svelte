<script lang="ts">
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>Tools · Local Network</title>
	<meta name="description" content="Tools for DJs from Local Network. Each one stands on its own and signs in with your Atmosphere account." />
</svelte:head>

<div class="container-page flex flex-col gap-8 py-10 sm:gap-12 sm:py-14">
	<SectionHeader level={1} eyebrow="Tools for DJs" tone="high" title="From the crate to the booth." lede="Each one stands on its own. They share your Atmosphere account and the records you publish on Local Network." />
	<ul class="m-0 flex list-none flex-wrap gap-4 p-0">
		{#each data.tools as tool (tool.id)}
			<Card as="li" padding="none" radius="2xl" class="flex min-w-0 flex-[1_1_26rem] flex-col overflow-hidden">
				<div class="flex flex-col gap-2 p-5 sm:p-6">
					<h2 class="m-0 flex flex-wrap items-baseline gap-x-2.5 text-heading font-semibold">
						<a href="/tools/{tool.id}" class="text-fg no-underline hover:underline">{tool.name}</a>
						<span class="font-mono text-caption font-normal text-fg-subtle">{tool.kind}</span>
					</h2>
					<p class="m-0 text-fg-muted">{tool.tagline}</p>
				</div>
				<ul class="m-0 flex flex-1 list-disc flex-col gap-1.5 border-t border-line bg-canvas py-4 pr-5 pl-10 text-small text-fg-soft marker:text-fg-subtle sm:pr-6 sm:pl-11">
					{#each tool.features as feature (feature)}<li>{feature}</li>{/each}
				</ul>
				<div class="border-t border-line px-5 py-4 sm:px-6">
					<Button size="sm" variant="secondary" href="/tools/{tool.id}">Install {tool.name}</Button>
				</div>
			</Card>
		{/each}
	</ul>
</div>
