<script lang="ts">
	let { tool, guides, current }: { tool: string; guides: { slug: string; title: string }[]; current?: string } = $props();
</script>

<nav aria-labelledby="guides-title" class="card border-ansi-bright-blue px-4 pt-2 pb-3">
	<h2 id="guides-title" class="section-title mb-2">Guides</h2>
	<ul>
		{#each guides as guide (guide.slug)}
			<li><a class="menu-link block px-1" href="/tools/{tool}/docs/{guide.slug}" aria-current={guide.slug === current ? 'page' : undefined}>{guide.title}</a></li>
		{/each}
	</ul>
</nav>
