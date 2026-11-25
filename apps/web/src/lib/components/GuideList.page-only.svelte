<script lang="ts">
	import { ChevronDown } from '@lucide/svelte';

	/** A tool's guides, beside the page on a wide screen and folded into a button above it below `lg`. */
	let { tool, guides, current }: { tool: string; guides: { slug: string; title: string }[]; current?: string } = $props();

	const id = $props.id();

	// Folded again on each guide, so after Previous, Next or Back the list doesn't cover the new page.
	let open = $derived.by(() => {
		void current;

		return false;
	});

	const here = $derived(guides.find((guide) => guide.slug === current));
</script>

<nav aria-label="Guides" class="max-lg:border-b max-lg:border-line max-lg:pb-3">
	<button type="button" aria-expanded={open} aria-controls="{id}-list" onclick={() => (open = !open)} class="menu-toggle lg:hidden">
		<span class="min-w-0 truncate">Guides{#if here}: {here.title}{/if}</span>
		<ChevronDown class={['ml-auto size-4 shrink-0 transition-transform motion-reduce:transition-none', open && 'rotate-180']} aria-hidden="true" />
	</button>
	<p class="mb-2 hidden px-3 section-title lg:block">Guides</p>
	<ul id="{id}-list" class={['flex-col gap-0.5 max-lg:pt-2 lg:flex', open ? 'flex' : 'hidden']}>
		{#each guides as guide (guide.slug)}
			<li><a class="nav-item" href="/tools/{tool}/docs/{guide.slug}" aria-current={guide.slug === current ? 'page' : undefined} onclick={() => (open = false)}>{guide.title}</a></li>
		{/each}
	</ul>
</nav>
