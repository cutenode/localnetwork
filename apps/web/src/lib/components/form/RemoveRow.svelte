<script lang="ts">
	import { Trash2 } from '@lucide/svelte';

	/** The button that drops one row of a repeated field, like a link or a fee. */
	let { label, onclick }: { label: string; onclick: () => void } = $props();
</script>

<button type="button" class="grid size-11 shrink-0 place-items-center rounded-lg text-fg-muted hover:bg-raised hover:text-fg" aria-label={label} {onclick}><Trash2 class="size-4" /></button>
