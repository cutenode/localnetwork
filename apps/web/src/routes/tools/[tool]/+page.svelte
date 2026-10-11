<script lang="ts">
	import GuideList from '#lib/components/GuideList.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const tool = $derived(data.tool);
</script>

<svelte:head>
	<title>{tool.name} · Local Network</title>
	<meta name="description" content="{tool.name}: {tool.tagline}" />
</svelte:head>

<div class="container-page py-12">
	<div class="max-w-3xl">
		<p class="eyebrow"><a class="menu-link" href="/tools">Tools</a></p>
		<h1 class="mt-2 text-5xl">{tool.name}</h1>
		<p class="mt-4 text-xl text-ansi-white">{tool.tagline}</p>
		<p class="muted mt-4 text-lg">{tool.summary}</p>
	</div>
	<div class="mt-12 grid gap-6 lg:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
		<div class="flex min-w-0 flex-col gap-6">
			<section aria-labelledby="features-title" class="card card-pad">
				<h2 id="features-title" class="section-title mb-3">What it does</h2>
				<ul class="space-y-1">
					{#each tool.features as feature (feature)}
						<li class="flex gap-2"><span class="text-ansi-bright-magenta" aria-hidden="true">*</span><span>{feature}</span></li>
					{/each}
				</ul>
			</section>
			<section aria-labelledby="install-title" class="card card-pad">
				<h2 id="install-title" class="section-title mb-3">Install</h2>
				{#if tool.install.note}<p>{tool.install.note}</p>{/if}
				<ol class="mt-3 list-decimal space-y-4 pl-7 marker:text-ansi-yellow">
					{#each tool.install.steps as step (step.text)}
						<li class="space-y-2">
							<p>{step.text}</p>
							{#if step.command}
								<!-- It scrolls sideways, so it takes focus to scroll from the keyboard. -->
								<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
								<pre class="code-block" tabindex="0">{step.command}</pre>
							{/if}
						</li>
					{/each}
				</ol>
			</section>
			{#each data.sections as section, i (section.title)}
				<section aria-labelledby="section-{i}" class="card card-pad">
					<h2 id="section-{i}" class="section-title mb-3">{section.title}</h2>
					<!-- Rendered on the server from the registry's Markdown, with raw HTML escaped. -->
					<div class="prose prose-ansi max-w-none">{@html section.html}</div>
				</section>
			{/each}
		</div>
		<div class="min-w-0">
			<GuideList tool={tool.id} guides={data.guides} />
		</div>
	</div>
</div>
