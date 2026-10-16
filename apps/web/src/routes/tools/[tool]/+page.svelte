<script lang="ts">
	import GuideList from '#lib/components/GuideList.svelte';
	import Card from '#lib/ui/Card.svelte';
	import CodeBlock from '#lib/ui/CodeBlock.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const tool = $derived(data.tool);
</script>

<svelte:head>
	<title>{tool.name} · Local Network</title>
	<meta name="description" content="{tool.name}: {tool.tagline}" />
</svelte:head>

<div class="container-page flex flex-col gap-8 py-10 sm:gap-12 sm:py-14">
	<div class="flex flex-col gap-5">
		<nav aria-label="Breadcrumb"><a class="menu-link text-small" href="/tools">Tools</a></nav>
		<SectionHeader level={1} eyebrow={tool.kind} tone="high" title={tool.name} lede={tool.tagline} />
		<p class="m-0 max-w-190 text-fg-soft">{tool.summary}</p>
	</div>
	<div class="grid gap-6 lg:grid-cols-[minmax(0,1fr)_18rem] lg:gap-10">
		<div class="flex min-w-0 flex-col gap-6">
			<Card as="section" padding="lg" radius="2xl" class="max-sm:p-5">
				<h2 class="m-0 text-heading font-semibold">What it does</h2>
				<ul class="m-0 mt-4 flex list-disc flex-col gap-2 pl-5 text-fg-soft marker:text-fg-subtle">
					{#each tool.features as feature (feature)}<li>{feature}</li>{/each}
				</ul>
			</Card>
			<Card as="section" padding="lg" radius="2xl" class="max-sm:p-5">
				<h2 class="m-0 text-heading font-semibold">Install</h2>
				{#if tool.install.note}<p class="m-0 mt-2 text-fg-muted">{tool.install.note}</p>{/if}
				<ol class="m-0 mt-5 flex list-none flex-col gap-5 p-0">
					{#each tool.install.steps as step, i (step.text)}
						<li class="flex gap-3.5">
							<span class="grid size-7 shrink-0 place-items-center rounded-full bg-raised font-mono text-caption text-fg-soft" aria-hidden="true">{i + 1}</span>
							<div class="flex min-w-0 flex-1 flex-col gap-3 pt-0.5">
								<p class="m-0">{step.text}</p>
								{#if step.command}<CodeBlock language="shell" code={step.command} />{/if}
							</div>
						</li>
					{/each}
				</ol>
			</Card>
			{#each data.sections as section, i (section.title)}
				<Card as="section" padding="lg" radius="2xl" class="max-sm:p-5">
					<h2 class="m-0 text-heading font-semibold">{section.title}</h2>
					<!-- Rendered on the server from the registry's Markdown, with raw HTML escaped. -->
					<div class="prose prose-site mt-3 max-w-none">{@html section.html}</div>
				</Card>
			{/each}
		</div>
		<div class="min-w-0 lg:sticky lg:top-6 lg:self-start">
			<GuideList tool={tool.id} guides={data.guides} />
		</div>
	</div>
</div>
