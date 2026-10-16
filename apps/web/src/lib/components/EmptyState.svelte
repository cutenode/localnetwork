<script lang="ts">
	import type { Snippet } from 'svelte';

	let { title, body, children }: { title: string; body?: string; children?: Snippet } = $props();
</script>

<div class="card flex flex-col items-center gap-2 px-5 py-10 text-center sm:py-12">
	<p class="text-emphasis font-semibold text-fg">{title}</p>
	{#if body}<p class="max-w-md text-small text-fg-muted">{body}</p>{/if}
	{#if children}<div class="mt-3">{@render children()}</div>{/if}
</div>
