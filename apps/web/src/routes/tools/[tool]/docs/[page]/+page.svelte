<script lang="ts">
	import WithGuides from '#lib/components/tools/WithGuides.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const at = $derived(data.guides.findIndex((guide) => guide.slug === data.doc.slug));

	const turns = $derived([
		{ rel: 'prev', label: 'Previous', guide: data.guides[at - 1] },
		{ rel: 'next', label: 'Next', guide: data.guides[at + 1] }
	]);
</script>

<div class="container-page flex flex-col gap-8 py-10 sm:gap-12 sm:py-14">
	<div class="flex flex-col gap-4">
		<nav aria-label="Breadcrumb" class="flex gap-2 text-small text-fg-subtle">
			<a class="menu-link" href="/tools">Tools</a><span aria-hidden="true">/</span><a class="menu-link" href="/tools/{data.tool.id}">{data.tool.name}</a>
		</nav>
		<h1 class="m-0 text-title font-semibold text-balance">{data.doc.title}</h1>
	</div>
	<WithGuides tool={data.tool.id} guides={data.guides} current={data.doc.slug}>
		<article class="flex min-w-0 flex-col gap-8">
			<!-- Rendered on the server from the tool's own Markdown, with raw HTML escaped. -->
			<div class="prose prose-site">{@html data.doc.html}</div>
			<p class="m-0 max-w-[65ch] border-t border-line pt-6 text-small text-fg-muted">
				<a class="link" href="https://github.com/cutenode/localnetwork/blob/main/{data.doc.path}">{data.doc.path}</a> on GitHub
			</p>
		</article>
		<nav aria-label="More guides" class="grid max-w-[65ch] gap-3 sm:grid-cols-2">
			{#each turns as { rel, label, guide } (rel)}
				{#if guide}
					<a {rel} href="/tools/{data.tool.id}/docs/{guide.slug}" class={['flex flex-col gap-0.5 rounded-xl bg-raised px-4 py-3 no-underline hover:bg-well', rel === 'next' && 'sm:col-start-2 sm:text-right']}>
						<span class="text-small text-fg-muted">{label}</span> <span class="font-semibold text-fg">{guide.title}</span>
					</a>
				{/if}
			{/each}
		</nav>
	</WithGuides>
</div>
