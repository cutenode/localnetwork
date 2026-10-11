<script lang="ts">
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const family = $derived([
		{ href: '/', name: 'Local Network', kind: 'Website', tagline: 'Promoters find local DJs, and DJs keep a profile they get booked from.' },
		...data.tools.map((tool) => ({ href: `/tools/${tool.id}`, name: tool.name, kind: tool.kind, tagline: tool.tagline }))
	]);
</script>

<svelte:head>
	<title>Tools · Local Network</title>
	<meta name="description" content="Tools for DJs from Local Network. Each one stands on its own and signs in with your Atmosphere account." />
</svelte:head>

<div class="container-page py-12">
	<div class="max-w-3xl">
		<p class="eyebrow">Tools</p>
		<h1 class="mt-2 text-5xl">Tools for DJs</h1>
		<p class="muted mt-4 text-lg">Each one stands on its own. They share your Atmosphere account and the records you publish.</p>
	</div>
	<ul class="mt-12 grid gap-6 md:grid-cols-3">
		{#each family as tool (tool.href)}
			<li class="card card-pad flex flex-col items-start gap-3">
				<span class="chip">{tool.kind}</span>
				<h2 class="text-2xl"><a class="link" href={tool.href}>{tool.name}</a></h2>
				<p class="muted text-sm">{tool.tagline}</p>
			</li>
		{/each}
	</ul>
</div>
