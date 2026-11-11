<script lang="ts">
	import { Eye } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import type { FlagPreview } from '#lib/types.ts';

	/** Says which flags this browser sees differently from everyone else, on every page, until they're cleared. */
	let { preview }: { preview: FlagPreview } = $props();

	const flags = $derived(Object.entries(preview));
</script>

<div role="status" class="notice flex items-start gap-2.5">
	<Eye aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-warning" />
	<!-- Beside the text when it fits, under it on a phone. -->
	<!-- Clear from any page and stay on it. Without JavaScript the browser lands on the flags list instead. -->
	<form method="POST" action="/admin/flags?/clearPreview" use:enhance={() => ({ update }) => update({ navigate: false })} class="flex min-w-0 flex-1 flex-wrap items-center justify-between gap-x-3 gap-y-1">
		<p class="m-0 min-w-0 flex-1 basis-56">
			Previewing {#each flags as [key, on], i (key)}{i > 0 ? ', ' : ''}<span class="font-mono text-small">{key}</span> {on ? 'on' : 'off'}{/each} in this browser only.
		</p>
		<button class="btn-ghost btn-sm -my-1">Clear</button>
	</form>
</div>
