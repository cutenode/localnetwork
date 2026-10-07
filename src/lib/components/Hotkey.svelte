<script lang="ts">
	import { hotkeys } from '#lib/hotkeys.svelte.ts';

	let { key }: { key: string } = $props();
</script>

{#if hotkeys.enabled}<span class="hotkey" aria-hidden="true">[<span>{key}</span>]</span>{/if}
