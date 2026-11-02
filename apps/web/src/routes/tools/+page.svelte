<script lang="ts">
	import Capture from '#lib/components/tools/Capture.svelte';
	import Button from '#lib/ui/Button.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<div class="container-page flex flex-col gap-4 pt-12 pb-4 sm:pt-20">
	<h1 class="m-0 text-display font-semibold text-balance">From the crate to the booth.</h1>
	<p class="m-0 max-w-2xl text-intro text-fg-muted">Tools for DJs. Each one stands on its own and signs in with your Atmosphere account.</p>
</div>

<!-- A band per tool, led by a capture of it at work; every other one has its capture on the left. -->
{#each data.tools as tool, i (tool.id)}
	<section aria-labelledby="{tool.id}-title" class="py-10 sm:py-16">
		<div class={['container-page grid items-center gap-8 lg:gap-14', i % 2 ? 'lg:grid-cols-[minmax(0,7fr)_minmax(0,5fr)]' : 'lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]']}>
			<div class={['flex flex-col gap-5', i % 2 && 'lg:order-2']}>
				<p class="m-0 text-small text-fg-muted">{tool.kind}</p>
				<h2 id="{tool.id}-title" class="m-0 text-title font-semibold"><a href="/tools/{tool.id}" class="text-fg no-underline hover:underline">{tool.name}</a></h2>
				<p class="m-0 text-lede text-fg-soft">{tool.tagline}</p>
				<ul class="m-0 flex list-none flex-col gap-2 p-0 text-fg-soft">
					{#each tool.lines as line (line)}<li class="flex gap-3"><span aria-hidden="true" class="mt-3 h-0.5 w-3 shrink-0 bg-cue-hot"></span>{line}</li>{/each}
				</ul>
				<div class="pt-2"><Button href="/tools/{tool.id}">See what {tool.name} does</Button></div>
			</div>
			<Capture capture={tool.capture} eager={i === 0} sizes="(min-width: 1280px) 700px, (min-width: 1024px) 56vw, 100vw" />
		</div>
	</section>
{/each}
