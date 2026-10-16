<script module lang="ts">
	/** The look every text input and select shares: the `input` class in theme.css. */
	export const control = 'input';
</script>

<script lang="ts">
	import type { Snippet } from 'svelte';

	/**
	 * A label, the control it names, and a hint or an error under it. The control gets the ids to
	 * point at through `describedby` and `invalid`.
	 */
	let {
		id,
		label,
		hint,
		error,
		children
	}: { id: string; label: string; hint?: string; error?: string; children: Snippet<[{ describedby: string | undefined; invalid: boolean }]> } = $props();

	const describedby = $derived(error ? `${id}-error` : hint ? `${id}-hint` : undefined);
</script>

<div class="flex flex-col gap-1.5">
	<label for={id} class="text-label font-medium text-fg-soft">{label}</label>
	{@render children({ describedby, invalid: Boolean(error) })}
	{#if error}
		<p id="{id}-error" class="m-0 text-label text-critical">{error}</p>
	{:else if hint}
		<p id="{id}-hint" class="m-0 text-label text-fg-subtle">{hint}</p>
	{/if}
</div>
