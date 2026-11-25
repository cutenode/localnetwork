<script lang="ts">
	import { Trash2 } from '@lucide/svelte';
	import { tick } from 'svelte';

	/**
	 * The button that drops one row of a repeated field, like a link or a fee. Its form hears an `input` once the row is
	 * gone, as it would from typing, so an editor that watches for changes (edit mode's) counts it as one.
	 */
	let { label, onclick }: { label: string; onclick: () => void } = $props();

	async function remove(event: MouseEvent & { currentTarget: HTMLButtonElement }) {
		const { form } = event.currentTarget;

		onclick();
		await tick();
		form?.dispatchEvent(new Event('input', { bubbles: true }));
	}
</script>

<button type="button" class="btn-icon" aria-label={label} onclick={remove}><Trash2 class="size-4" /></button>
