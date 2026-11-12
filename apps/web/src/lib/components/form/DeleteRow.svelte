<script lang="ts">
	import { Trash2 } from '@lucide/svelte';

	/** The foot of an editor: deleting posts to `action`, one step after `warning` says what goes with it. */
	let { action, label, note, warning }: { action: string; label: string; note: string; warning: string } = $props();

	let confirming = $state(false);
</script>

<form method="POST" {action} class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
	<p class="m-0 text-small text-fg-muted">{confirming ? warning : note}</p>
	<div class="flex shrink-0 flex-wrap gap-2">
		{#if confirming}
			<button type="button" class="btn-ghost btn-sm" onclick={() => (confirming = false)}>Keep it</button>
			<button class="btn-danger btn-sm"><Trash2 class="size-4" />{label}</button>
		{:else}
			<button type="button" class="btn-ghost btn-sm" onclick={() => (confirming = true)}><Trash2 class="size-4" />Delete…</button>
		{/if}
	</div>
</form>
