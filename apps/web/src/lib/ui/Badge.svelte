<script lang="ts">
	import type { Snippet } from 'svelte';

	/** An announcement pill: a short mono label, then one line of news. */
	let { label, href, children }: { label: string; href?: string; children: Snippet } = $props();
</script>

<svelte:element this={href ? 'a' : 'span'} {href} class="inline-flex items-center gap-2.5 rounded-full border border-line-strong bg-raised py-1.5 pr-3.5 pl-1.5 text-label text-fg-soft no-underline transition-colors hover:border-fg-subtle">
	<span class="rounded-full bg-link-wash px-2 py-0.5 font-mono text-caption text-link">{label}</span>
	{@render children()}
</svelte:element>
