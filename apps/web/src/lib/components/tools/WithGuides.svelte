<script lang="ts">
	import type { Snippet } from 'svelte';
	import GuideList from '../GuideList.svelte';

	/** A tool page or guide with the tool's guides on its left, folded above it below `lg`. */
	let { tool, guides, current, children }: { tool: string; guides: { slug: string; title: string }[]; current?: string; children: Snippet } = $props();
</script>

<div class="grid gap-6 lg:grid-cols-[14rem_minmax(0,1fr)] lg:gap-14">
	<div class="min-w-0 lg:sticky lg:top-6 lg:max-h-[calc(100vh-3rem)] lg:self-start lg:overflow-y-auto">
		<GuideList {tool} {guides} {current} />
	</div>
	<div class="flex min-w-0 flex-col gap-12">{@render children()}</div>
</div>
