<script lang="ts">
	import type { Snippet } from 'svelte';
	import GuideList from '../GuideList.svelte';

	/**
	 * A tool page or guide with the tool's guides on its left, folded into the page below `lg`. The
	 * fold comes first there, or after `lead` when there's one (a tool page's Install), so the page
	 * opens on what most people came for.
	 */
	let { tool, guides, current, lead, children }: { tool: string; guides: { slug: string; title: string }[]; current?: string; lead?: Snippet; children: Snippet } = $props();
</script>

<div class="grid gap-y-6 lg:gap-y-12 lg:grid-cols-[14rem_minmax(0,1fr)] lg:gap-x-14">
	{#if lead}<div class="flex min-w-0 flex-col gap-12 lg:col-start-2 lg:row-start-1">{@render lead()}</div>{/if}
	<div class={['min-w-0 lg:sticky lg:top-6 lg:col-start-1 lg:row-start-1 lg:max-h-[calc(100vh-3rem)] lg:self-start lg:overflow-y-auto', lead && 'lg:row-span-2']}>
		<GuideList {tool} {guides} {current} />
	</div>
	<div class="flex min-w-0 flex-col gap-12 lg:col-start-2">{@render children()}</div>
</div>
