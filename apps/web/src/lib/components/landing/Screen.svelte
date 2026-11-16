<script lang="ts">
	import type { Snippet } from 'svelte';

	/**
	 * A screen from the site on a landing page, under the path it lives at. `example` marks one filled with made-up
	 * data, for screens that only show signed in; the rest show what the directory has now.
	 */
	let { path, example = false, caption, children }: { path: string; example?: boolean; caption: Snippet; children: Snippet } = $props();
</script>

<figure class="m-0 flex min-w-0 flex-col gap-3">
	<div class="card overflow-hidden">
		<p class="m-0 flex items-center justify-between gap-3 border-b border-line bg-canvas px-5 py-2 font-mono text-caption text-fg-subtle">
			<span class="truncate">{path}</span>
			{#if example}<span class="pill shrink-0 bg-well text-fg-muted">Example</span>{/if}
		</p>
		<div class="p-4 sm:p-5">{@render children()}</div>
	</div>
	<figcaption class="text-small text-fg-muted">{@render caption()}</figcaption>
</figure>
