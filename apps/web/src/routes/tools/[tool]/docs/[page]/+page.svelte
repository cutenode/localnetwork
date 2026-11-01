<script lang="ts">
	import GuideList from '#lib/components/GuideList.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<div class="container-page flex flex-col gap-8 py-10 sm:gap-12 sm:py-14">
	<div class="flex flex-col gap-4">
		<nav aria-label="Breadcrumb" class="flex gap-2 text-small text-fg-subtle">
			<a class="menu-link" href="/tools">Tools</a><span aria-hidden="true">/</span><a class="menu-link" href="/tools/{data.tool.id}">{data.tool.name}</a>
		</nav>
		<h1 class="m-0 text-title font-semibold text-balance">{data.doc.title}</h1>
	</div>
	<!-- The guide list sits beside the guide on a wide screen and above it on a phone. -->
	<div class="grid gap-8 lg:grid-cols-[16rem_minmax(0,1fr)] lg:gap-14">
		<div class="min-w-0 lg:sticky lg:top-6 lg:max-h-[calc(100vh-3rem)] lg:self-start lg:overflow-y-auto">
			<GuideList tool={data.tool.id} guides={data.guides} current={data.doc.slug} />
		</div>
		<article class="flex min-w-0 flex-col gap-8">
			<!-- Rendered on the server from the tool's own Markdown, with raw HTML escaped. -->
			<div class="prose prose-site">{@html data.doc.html}</div>
			<p class="m-0 max-w-[65ch] border-t border-line pt-6 text-small text-fg-muted">
				<a class="link" href="https://github.com/cutenode/localnetwork/blob/main/{data.doc.path}">{data.doc.path}</a> on GitHub
			</p>
		</article>
	</div>
</div>
