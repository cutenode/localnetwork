<script lang="ts">
	import type { Snippet } from 'svelte';

	let { title, body, children }: { title: string; body?: string; children?: Snippet } = $props();
</script>

<div class="card card-pad flex flex-col items-center gap-2 border-ansi-bright-blue py-10 text-center">
	<p class="text-2xl text-ansi-white">{title}</p>
	{#if body}<p class="muted max-w-md">{body}</p>{/if}
	{#if children}<div class="mt-2">{@render children()}</div>{/if}
</div>
