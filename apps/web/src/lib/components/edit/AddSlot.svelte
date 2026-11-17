<script lang="ts">
	import { Plus } from '@lucide/svelte';

	/**
	 * Where a section would be in edit mode while it has nothing in it yet, so an empty page still has the real page's
	 * layout and every section can be found. `hint` says what it's for, in a line.
	 */
	let { label, hint, onclick }: { label: string; hint?: string; onclick?: () => void } = $props();
</script>

<button type="button" {onclick} class="flex w-full items-start gap-3 rounded-xl border border-dashed border-line-strong p-5 text-left text-fg-muted transition-colors hover:border-fg-subtle hover:text-fg">
	<Plus aria-hidden="true" class="mt-0.5 size-4 shrink-0" />
	<span class="flex min-w-0 flex-col gap-1">
		<span class="text-small font-medium text-fg">{label}</span>
		{#if hint}<span class="text-label text-fg-subtle">{hint}</span>{/if}
	</span>
</button>
