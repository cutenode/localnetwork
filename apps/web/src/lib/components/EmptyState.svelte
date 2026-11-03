<script lang="ts">
	import type { Snippet } from 'svelte';

	/** What's missing, why, and the next step (`children`). `framed: false` drops the card, for one already inside a card. */
	let { title, body, framed = true, children }: { title: string; body?: string; framed?: boolean; children?: Snippet } = $props();
</script>

<div class={[framed && 'card', 'flex flex-col items-center gap-2 px-5 py-10 text-center sm:py-12']}>
	<p class="text-emphasis font-semibold text-fg">{title}</p>
	{#if body}<p class="max-w-md text-small text-fg-muted">{body}</p>{/if}
	{#if children}<div class="mt-3">{@render children()}</div>{/if}
</div>
