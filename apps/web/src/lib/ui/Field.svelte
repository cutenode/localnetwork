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

<div>
	<label for={id} class="label">{label}</label>
	{@render children({ describedby, invalid: Boolean(error) })}
	{#if error}
		<p id="{id}-error" class="hint text-critical">{error}</p>
	{:else if hint}
		<p id="{id}-hint" class="hint">{hint}</p>
	{/if}
</div>
