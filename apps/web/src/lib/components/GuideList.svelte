<script lang="ts">
	let { tool, guides, current }: { tool: string; guides: { slug: string; title: string }[]; current?: string } = $props();
</script>

<nav aria-labelledby="guides-title" class="card p-4">
	<h2 id="guides-title" class="section-title mb-2 px-2">Guides</h2>
	<ul class="flex flex-col gap-0.5">
		{#each guides as guide (guide.slug)}
			<li><a class="menu-link block rounded-md px-2 py-1.5 text-small hover:bg-raised aria-[current=page]:bg-raised aria-[current=page]:font-semibold" href="/tools/{tool}/docs/{guide.slug}" aria-current={guide.slug === current ? 'page' : undefined}>{guide.title}</a></li>
		{/each}
	</ul>
</nav>
