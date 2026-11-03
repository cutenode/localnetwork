<script lang="ts">
	import HotCues from '#lib/components/tools/HotCues.svelte';
	import { WithGuides } from '#lib/components/tools/index.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import CodeBlock from '#lib/ui/CodeBlock.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const tool = $derived(data.tool);
</script>

<div class="container-page flex flex-col gap-12 py-10 sm:gap-16 sm:py-14">
	<div class="flex flex-col gap-5">
		<nav aria-label="Breadcrumb"><a class="menu-link text-small" href="/tools">Tools</a></nav>
		<div class="flex flex-wrap items-end justify-between gap-x-10 gap-y-6">
			<div class="flex max-w-3xl min-w-0 flex-col gap-3">
				<p class="m-0 text-small text-fg-muted">{tool.kind}</p>
				<!-- The name is one long word: with text enlarged on a phone it breaks rather than runs off the screen. -->
				<h1 class="m-0 text-display font-semibold wrap-anywhere">{tool.name}</h1>
				<p class="m-0 text-intro text-fg-muted">{tool.tagline}</p>
				<p class="m-0 max-w-[65ch] text-fg-soft">{tool.summary}</p>
			</div>
			<div class="flex max-w-xs min-w-0 flex-col items-start gap-2">
				<!-- Wraps for the same reason. -->
				<Button href="#install" class="max-w-full text-center whitespace-normal">How to install it</Button>
				{#if tool.status}<p class="m-0 text-small text-fg-muted">{tool.status}</p>{/if}
			</div>
		</div>
	</div>

	<!-- Remounted for each tool, so a cue picked on one doesn't carry over to the next. -->
	{#key tool.id}<HotCues tool={tool.name} cues={tool.cues} track={data.track} look={data.look} />{/key}

	<WithGuides tool={tool.id} guides={data.guides} lead={install}>
		{#each data.sections as section (section.title)}
			<section class="flex flex-col gap-3">
				<h2 class="m-0 text-heading font-semibold">{section.title}</h2>
				<!-- Rendered on the server from the registry's Markdown, with raw HTML escaped. -->
				<div class="prose prose-site max-w-[38rem]">{@html section.html}</div>
			</section>
		{/each}
		<section aria-labelledby="next-title" class="flex flex-col gap-6 border-t border-line pt-10">
			<div class="flex flex-wrap items-center justify-between gap-4">
				<h2 id="next-title" class="m-0 text-heading font-semibold">Ready to try {tool.name}?</h2>
				<Button href="#install" variant="secondary">Install it</Button>
			</div>
			{#if data.others.length}
				<ul aria-label="More tools" class="m-0 flex list-none flex-col gap-3 p-0">
					{#each data.others as other (other.id)}
						<li><Card href="/tools/{other.id}" tone="outline" class="flex flex-col gap-1"><span class="font-semibold">{other.name}</span><span class="text-pretty text-fg-muted">{other.tagline}</span></Card></li>
					{/each}
				</ul>
			{/if}
		</section>
	</WithGuides>
</div>

{#snippet install()}
		<section id="install" aria-labelledby="install-title" class="flex scroll-mt-6 flex-col gap-5">
			<div class="flex flex-col gap-2">
				<h2 id="install-title" class="m-0 text-heading font-semibold">Install</h2>
				{#if tool.install.note}<p class="m-0 max-w-[38rem] text-pretty text-fg-muted">{tool.install.note}</p>{/if}
			</div>
			<ol class="m-0 flex list-none flex-col gap-5 p-0">
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
		</section>
{/snippet}
