<script lang="ts">
	import GuideList from '#lib/components/GuideList.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>{data.doc.title} · {data.tool.name} · Local Network</title>
</svelte:head>

<div class="container-page py-12">
	<p class="eyebrow"><a class="menu-link" href="/tools/{data.tool.id}">{data.tool.name}</a></p>
	<h1 class="mt-2 text-4xl">{data.doc.title}</h1>
	<div class="mt-8 grid gap-10 lg:grid-cols-[minmax(0,1fr)_16rem]">
		<article class="min-w-0">
			<!-- Rendered on the server from the tool's own Markdown, with raw HTML escaped. -->
			<div class="prose prose-ansi max-w-3xl">{@html data.doc.html}</div>
			<p class="muted mt-10">
				<a class="link" href="https://github.com/cutenode/localnetwork/blob/main/{data.doc.path}">{data.doc.path}</a> on GitHub
			</p>
		</article>
		<div class="min-w-0 lg:sticky lg:top-6 lg:self-start">
			<GuideList tool={data.tool.id} guides={data.guides} current={data.doc.slug} />
		</div>
	</div>
</div>
